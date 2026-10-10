use std::{
    env, fs,
    io::{self, IsTerminal},
    path::Path,
    process::ExitCode,
};

use my_blog_api::{
    config::Config,
    database,
    posts::{
        dto::{CreatePostDto, UpdatePostDto},
        model::PostStatus,
        repository as post_repo,
        service as post_service,
        slug,
    },
    users::{
        model::UserRole,
        service::{self, SeedResult},
    },
};

fn read_secret(prompt: &str) -> Result<String, String> {
    if let Some(env_pass) = env::var("BLOG_ADMIN_PASSWORD")
        .ok()
        .filter(|p| !p.trim().is_empty())
    {
        return Ok(env_pass);
    }
    if io::stdin().is_terminal() {
        let first = rpassword::prompt_password(prompt).map_err(|e| e.to_string())?;
        let second = rpassword::prompt_password("Confirm password: ").map_err(|e| e.to_string())?;
        if first != second {
            return Err("passwords do not match".into());
        }
        Ok(first)
    } else {
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }
}

fn get_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn get_password(args: &[String], prompt: &str) -> Result<String, String> {
    if let Some(pass) = get_arg(args, "--password") {
        if !pass.trim().is_empty() {
            return Ok(pass);
        }
    }
    read_secret(prompt)
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" || args[1] == "help" {
        println!("Usage: blog-admin <command> [options]");
        println!(
            "Commands: seed-owner, seed-posts, user-create, user-disable, user-reset-password, user-set-role, user-update-email"
        );
        return ExitCode::SUCCESS;
    }

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            return ExitCode::from(1);
        }
    };

    let pool = match database::connect(&config.database_url, config.database_max_connections).await
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Database error: {e}");
            return ExitCode::from(1);
        }
    };

    let command = args[1].as_str();
    let result = match command {
        "seed-owner" => handle_seed_owner(&pool, &args).await,
        "seed-posts" => handle_seed_posts(&pool).await,
        "user-create" => handle_user_create(&pool, &args).await,
        "user-disable" => handle_user_disable(&pool, &args).await,
        "user-reset-password" => handle_user_reset_password(&pool, &args).await,
        "user-set-role" => handle_user_set_role(&pool, &args).await,
        "user-update-email" => handle_user_update_email(&pool, &args).await,
        _ => {
            eprintln!("Unknown command: {command}");
            Err("unknown command".into())
        }
    };

    pool.close().await;

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn handle_seed_owner(pool: &sqlx::SqlitePool, args: &[String]) -> Result<(), String> {
    let email = match get_arg(args, "--email") {
        Some(e) => e,
        None => prompt_text("Owner email: ")?,
    };
    let display_name = match get_arg(args, "--name") {
        Some(n) => n,
        None => prompt_text("Owner display name: ")?,
    };
    let password = get_password(args, "Owner password: ")?;

    match service::seed_owner(pool, &email, &display_name, &password).await {
        Ok(SeedResult::Created(id)) => {
            println!("Owner successfully created with id: {id}");
            Ok(())
        }
        Ok(SeedResult::AlreadyExists(id)) => {
            println!("Owner already exists with id: {id}");
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn handle_seed_posts(pool: &sqlx::SqlitePool) -> Result<(), String> {
    let owner_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM users WHERE role = 'owner' AND status = 'active' ORDER BY id ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let owner_id = match owner_id {
        Some(id) => id,
        None => return Err("No active owner found. Please run `seed-owner` first.".into()),
    };

    let seeds_dir = Path::new("seeds");
    if !seeds_dir.is_dir() {
        return Err("Seeds directory 'seeds' not found.".into());
    }

    let mut entries: Vec<_> = fs::read_dir(seeds_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let content = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
        let seed = match parse_seed_markdown(&content) {
            Some(s) => s,
            None => {
                eprintln!("Skipping invalid seed format: {:?}", entry.path());
                continue;
            }
        };

        let base_slug = slug::generate_slug(&seed.title);
        if let Ok(Some(existing)) = post_repo::find_post_by_slug(pool, &base_slug).await {
            let update_dto = UpdatePostDto {
                title: Some(seed.title.clone()),
                summary: Some(seed.summary.clone()),
                content_md: Some(seed.content_md.clone()),
                featured_image_media_id: seed.cover.clone(),
                status: Some(seed.status),
                tags: Some(seed.tags.clone()),
                scheduled_for: None,
                book_color: seed.book_color.clone(),
                version: existing.version,
            };
            match post_service::update_post(pool, existing.id, update_dto).await {
                Ok(_) => println!("Updated seed post: '{}' (slug: {})", seed.title, base_slug),
                Err(e) => eprintln!("Failed to update post '{}': {e}", seed.title),
            }
        } else {
            let create_dto = CreatePostDto {
                title: seed.title.clone(),
                summary: Some(seed.summary.clone()),
                content_md: seed.content_md.clone(),
                featured_image_media_id: seed.cover.clone(),
                status: Some(seed.status),
                tags: Some(seed.tags.clone()),
                scheduled_for: None,
                book_color: seed.book_color.clone(),
            };
            match post_service::create_post(pool, owner_id, create_dto).await {
                Ok(c) => println!("Created seed post: '{}' (slug: {})", c.post.title, c.post.slug),
                Err(e) => eprintln!("Failed to create post '{}': {e}", seed.title),
            }
        }
    }

    Ok(())
}

struct SeedPost {
    title: String,
    summary: String,
    content_md: String,
    cover: Option<String>,
    tags: Vec<String>,
    book_color: Option<String>,
    status: PostStatus,
}

fn parse_seed_markdown(raw: &str) -> Option<SeedPost> {
    if !raw.starts_with("---") {
        return None;
    }
    let parts: Vec<&str> = raw.splitn(3, "---").collect();
    if parts.len() < 3 {
        return None;
    }
    let meta = parts[1];
    let body = parts[2].trim();

    let mut title = String::new();
    let mut summary = String::new();
    let mut cover = None;
    let mut tags = Vec::new();
    let mut book_color = None;
    let mut status = PostStatus::Published;

    for line in meta.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim().trim_matches('"').trim_matches('\'');
            match key {
                "title" => title = val.to_string(),
                "summary" => summary = val.to_string(),
                "cover" => cover = Some(val.to_string()),
                "tags" => {
                    tags = val
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "book_color" => book_color = Some(val.to_string()),
                "status" if val == "draft" => status = PostStatus::Draft,
                _ => {}
            }
        }
    }

    if title.is_empty() {
        return None;
    }

    Some(SeedPost {
        title,
        summary,
        content_md: body.to_string(),
        cover,
        tags,
        book_color,
        status,
    })
}

async fn handle_user_create(pool: &sqlx::SqlitePool, args: &[String]) -> Result<(), String> {
    let email = get_arg(args, "--email").ok_or("missing --email")?;
    let name = get_arg(args, "--name").ok_or("missing --name")?;
    let role_str = get_arg(args, "--role").unwrap_or_else(|| "author".into());
    let role = role_str.parse::<UserRole>().map_err(|e| e.to_string())?;
    let password = get_password(args, "User password: ")?;

    let id = service::create_user(pool, &email, &name, &password, role)
        .await
        .map_err(|e| e.to_string())?;

    println!("User created with id: {id}");
    Ok(())
}

async fn handle_user_disable(pool: &sqlx::SqlitePool, args: &[String]) -> Result<(), String> {
    let id_str = get_arg(args, "--id").ok_or("missing --id")?;
    let id = id_str.parse::<i64>().map_err(|_| "invalid --id")?;
    service::disable_user(pool, id)
        .await
        .map_err(|e| e.to_string())?;
    println!("User {id} disabled successfully");
    Ok(())
}

async fn handle_user_reset_password(
    pool: &sqlx::SqlitePool,
    args: &[String],
) -> Result<(), String> {
    let id = if let Some(id_str) = get_arg(args, "--id") {
        id_str.parse::<i64>().map_err(|_| "invalid --id")?
    } else if let Some(email) = get_arg(args, "--email") {
        let normalized = my_blog_api::users::service::normalize_email(&email).map_err(|e| e.to_string())?;
        let user = my_blog_api::users::repository::find_by_normalized_email(pool, &normalized)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("user with email '{email}' not found"))?;
        user.id
    } else {
        return Err("missing --id or --email".into());
    };
    let new_password = get_password(args, "New password: ")?;
    service::reset_password(pool, id, &new_password)
        .await
        .map_err(|e| e.to_string())?;
    println!("Password reset successfully for user {id}");
    Ok(())
}

async fn handle_user_set_role(pool: &sqlx::SqlitePool, args: &[String]) -> Result<(), String> {
    let id_str = get_arg(args, "--id").ok_or("missing --id")?;
    let id = id_str.parse::<i64>().map_err(|_| "invalid --id")?;
    let role_str = get_arg(args, "--role").ok_or("missing --role")?;
    let role = role_str.parse::<UserRole>().map_err(|e| e.to_string())?;
    service::set_user_role(pool, id, role)
        .await
        .map_err(|e| e.to_string())?;
    println!("User {id} role updated successfully to {role}");
    Ok(())
}

async fn handle_user_update_email(pool: &sqlx::SqlitePool, args: &[String]) -> Result<(), String> {
    let id_str = get_arg(args, "--id").ok_or("missing --id")?;
    let id = id_str.parse::<i64>().map_err(|_| "invalid --id")?;
    let new_email = get_arg(args, "--new-email")
        .or_else(|| get_arg(args, "--email"))
        .ok_or("missing --new-email")?;
    service::update_user_email(pool, id, &new_email)
        .await
        .map_err(|e| e.to_string())?;
    println!("User {id} email updated successfully to {new_email}");
    Ok(())
}

fn prompt_text(prompt: &str) -> Result<String, String> {
    if io::stdin().is_terminal() {
        eprint!("{prompt}");
    }
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        return Err("input cannot be empty".into());
    }
    Ok(trimmed)
}
