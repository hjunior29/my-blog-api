use std::{
    env,
    io::{self, IsTerminal},
    process::ExitCode,
};

use my_blog_api::{
    config::Config,
    database,
    posts::{dto::CreatePostDto, model::PostStatus, service as post_service},
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
            "Commands: seed-owner, seed-posts, user-create, user-disable, user-reset-password, user-set-role"
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

    struct SamplePost {
        title: &'static str,
        summary: &'static str,
        content_md: &'static str,
        status: PostStatus,
        tags: &'static [&'static str],
    }

    let samples: [SamplePost; 6] = [
        SamplePost {
            title: "Construindo Sistemas com Rust e SolidJS: Desempenho e Simplicidade",
            summary: "Uma reflexao pratica sobre a combinacao de um backend robusto em Rust com o modelo reativo de grano fino do SolidJS.",
            content_md: "Ao desenhar a arquitetura deste blog pessoal, o objetivo central foi unir **desempenho intransigente** e uma experiencia de desenvolvimento simples.\n\n### Por que Rust no Backend?\n\nO uso do ecossistema Axum e Tokio com SQLite embutido nos proporciona:\n- **Consumo de memoria infimo:** O processo consome poucos megabytes em repouso.\n- **Concorrencia segura:** Verificacao estrita em tempo de compilacao.\n- **Tipagem de ponta a ponta:** DTOs com validacoes claras e sem suposicoes.\n\n```rust\n// Exemplo conceitual do pipeline\npub async fn get_post_by_slug(pool: &SqlitePool, slug: &str) -> Result<Post, PostError> {\n    repository::find_by_slug(pool, slug).await\n}\n```\n\n### A Escolha do SolidJS\n\nDiferente de frameworks com Virtual DOM pesados, o SolidJS compila diretamente para nos reativos do DOM:\n1. Sem reconciliacao continua e sem overhead de diffing.\n2. Estado granular que atualiza estritamente o no afetado.\n3. Bundle inicial compacto (< 60 KiB gzipped).\n\nO resultado e uma leitura fluida e navegacao instantanea.",
            status: PostStatus::Published,
            tags: &["Rust", "SolidJS", "Arquitetura", "Web"],
        },
        SamplePost {
            title: "Principios de um Design System Editorial Focado em Tipografia",
            summary: "Como equilibrar atmosfera estetica acolhedora, contraste WCAG AAA e tipografia proporcional inspirada em livros e papel.",
            content_md: "O design visual de um blog tecnico e reflexivo nao deve competir com o conteudo, mas sim acolhe-lo.\n\n> \"A boa tipografia e como um vidro transparente: voce le o que esta por tras sem perceber a lente.\"\n\n### Escolhas Tipograficas\n\nAdotamos uma hierarquia que valoriza a legibilidade:\n- **Newsreader:** Uma serifa editorial elegante para titulos e destaques.\n- **Manrope:** Sans-serif geometrica para o corpo de texto e controles.\n- **Geist Mono:** Monospace cirurgica para metadados e codigo.\n\n### Cores e Texturas\n\nTrabalhamos com uma paleta inspirada em materiais fisicos:\n- Fundo papel quente (*warm paper*) que evita a fadiga do branco puro.\n- Tinta profunda (*deep ink*) mantendo contraste adequado em temas claro e escuro.\n- Acentos terracota sutis para guiar a atencao com harmonia.",
            status: PostStatus::Published,
            tags: &["Design System", "CSS", "Tipografia", "Acessibilidade"],
        },
        SamplePost {
            title: "SQLite em Producao: Por Que Bancos Embutidos Fazem Sentido",
            summary: "Desmistificando o uso do SQLite em servicos web modernos com WAL mode e transacoes ACID locais.",
            content_md: "Por muitos anos, a convencao padrao para qualquer aplicacao web foi subir um servidor de banco de dados separado, mesmo para sites de trafego moderado ou blogs pessoais.\n\n### As Vantagens do SQLite Moderno\n\nCom as opcoes corretas, o SQLite e uma escolha extraordinaria:\n- **Zero latencia de rede:** Consultas executam no mesmo processo, sem round-trip TCP.\n- **WAL (Write-Ahead Logging):** Leitores concorrentes nao bloqueiam escritores.\n- **Backup simplificado:** Um unico arquivo persistente que pode ser versionado ou copiado.\n- **FTS5 Integrado:** Busca textual completa sem servicos externos pesados.\n\n### Configuracao Recomendada\n\n```sql\nPRAGMA journal_mode = WAL;\nPRAGMA synchronous = FULL;\nPRAGMA foreign_keys = ON;\nPRAGMA busy_timeout = 5000;\n```\n\nEssa simplicidade reduz a complexidade operacional e os custos a praticamente zero.",
            status: PostStatus::Published,
            tags: &["SQLite", "Banco de Dados", "Rust", "Performance"],
        },
        SamplePost {
            title: "Seguranca Pragmatica em APIs: Sessoes HttpOnly sem Complexidade",
            summary: "A estrategia de tokens JWT de curta duracao, cookies seguros e protecao contra CSRF em arquiteturas modernas.",
            content_md: "A seguranca nao deve ser nem negligenciada nem transformada em um labirinto impraticavel.\n\n### Estrategia de Sessao Adotada\n\nOptamos pelo equilibrio entre robustez e facilidade operacional:\n1. **Cookies HttpOnly e SameSite=Lax:** O JavaScript do navegador nunca tem acesso direto aos tokens de autenticacao.\n2. **Access Token curto + Refresh Rotativo:** O access token expira rapidamente e a rotatividade detecta tentativas de reuso.\n3. **Verificacao de Origem e CSRF:** Qualquer mutacao valida a origem exata e exige token CSRF em memoria.\n\nDessa forma, mantemos o blog seguro sem depender de middlewares opacos.",
            status: PostStatus::Published,
            tags: &["Seguranca", "Backend", "Web"],
        },
        SamplePost {
            title: "Otimizando a Performance Web do Inicio ao Fim",
            summary: "Metricas Core Web Vitals, carregamento sob demanda e tecnicas para entregar paginas em menos de 100 milissegundos.",
            content_md: "Velocidade e uma funcionalidade essencial para qualquer produto digital.\n\n### Estrategias Essenciais\n\nPara atingir pontuacoes maximas nos Core Web Vitals (LCP, INP, CLS):\n- **Eliminacao de CSS nao utilizado:** Menos de 60 KB de estilos totais.\n- **SVG Inlined e Otimizado:** Icones leves sem requisicoes de rede extras.\n- **Fontes com preload:** Evita flashes de texto invisivel (FOIT).\n- **Zero Empty States:** Cada transicao de tela possui esqueletos e feedbacks claros.\n\nQuando cada milissegundo conta, a leitura ganha fluidez natural.",
            status: PostStatus::Published,
            tags: &["Performance", "Web", "JavaScript"],
        },
        SamplePost {
            title: "Proximos Passos: Suporte a Midias e Workers de Agendamento",
            summary: "Rascunho de planejamento das proximas melhorias no blog, incluindo upload de capas e rotinas em background.",
            content_md: "Anotacoes e roadmap para os proximos ciclos de desenvolvimento:\n\n- [ ] Upload multipart autenticado para imagens de capa e ilustracoes.\n- [ ] Worker em background para transicao automatica de posts agendados.\n- [ ] Feeds RSS e Atom para distribuicao aberta de conteudo.\n\n*Este artigo e um rascunho de trabalho visivel apenas no painel administrativo.*",
            status: PostStatus::Draft,
            tags: &["Roadmap", "DevOps"],
        },
    ];

    for item in samples {
        let base_slug = my_blog_api::posts::slug::generate_slug(item.title);
        if my_blog_api::posts::repository::slug_exists(pool, &base_slug)
            .await
            .unwrap_or(false)
        {
            println!("Post already exists, skipping: '{}'", item.title);
            continue;
        }

        let dto = CreatePostDto {
            title: item.title.to_string(),
            summary: Some(item.summary.to_string()),
            content_md: item.content_md.to_string(),
            featured_image_media_id: None,
            status: Some(item.status),
            tags: Some(item.tags.iter().map(|&s| s.to_string()).collect()),
            scheduled_for: None,
        };

        match post_service::create_post(pool, owner_id, dto).await {
            Ok(created) => {
                println!(
                    "Created post: '{}' (slug: {}, status: {:?})",
                    created.post.title, created.post.slug, created.post.status
                );
            }
            Err(e) => {
                eprintln!("Failed to create post '{}': {e}", item.title);
            }
        }
    }

    Ok(())
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
