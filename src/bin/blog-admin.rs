use std::{
    env,
    io::{self, IsTerminal},
    process::ExitCode,
};

use my_blog_api::{
    config::Config,
    database,
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

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" || args[1] == "help" {
        println!("Usage: blog-admin <command> [options]");
        println!(
            "Commands: seed-owner, user-create, user-disable, user-reset-password, user-set-role"
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
        "user-create" => handle_user_create(&pool, &args).await,
        "user-disable" => handle_user_disable(&pool, &args).await,
        "user-reset-password" => handle_user_reset_password(&pool, &args).await,
        "user-set-role" => handle_user_set_role(&pool, &args).await,
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
    let password = read_secret("Owner password: ")?;

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

async fn handle_user_create(pool: &sqlx::SqlitePool, args: &[String]) -> Result<(), String> {
    let email = get_arg(args, "--email").ok_or("missing --email")?;
    let name = get_arg(args, "--name").ok_or("missing --name")?;
    let role_str = get_arg(args, "--role").unwrap_or_else(|| "author".into());
    let role = role_str.parse::<UserRole>().map_err(|e| e.to_string())?;
    let password = read_secret("User password: ")?;

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
    let id_str = get_arg(args, "--id").ok_or("missing --id")?;
    let id = id_str.parse::<i64>().map_err(|_| "invalid --id")?;
    let new_password = read_secret("New password: ")?;
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
