use my_blog_api::{
    database,
    posts::{
        PostStatus,
        dto::{CreatePostDto, UpdatePostDto},
        service as post_service,
    },
    users::{model::UserRole, service as user_service},
};
use sqlx::SqlitePool;

async fn setup_test_db() -> (SqlitePool, i64, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("posts_test.db");
    let url = format!("sqlite://{}", db_path.display());
    let pool = database::connect(&url, 2).await.unwrap();

    let author_id = user_service::create_user(
        &pool,
        "writer@example.com",
        "Staff Writer",
        "SecurePassword123!",
        UserRole::Author,
    )
    .await
    .unwrap();

    (pool, author_id, dir)
}

#[tokio::test]
async fn creates_post_with_rendered_html_and_tags() {
    let (pool, author_id, _dir) = setup_test_db().await;

    let dto = CreatePostDto {
        title: "Primeiro Post do Blog!".to_string(),
        summary: Some("Resumo do primeiro post.".to_string()),
        content_md: "# Olá Mundo\n\nEste é o primeiro post com [link](https://rust-lang.org)."
            .to_string(),
        featured_image_media_id: None,
        status: Some(PostStatus::Draft),
        tags: Some(vec![
            "Rust".to_string(),
            "Web Development".to_string(),
            "rust".to_string(),
        ]),
        scheduled_for: None,
    };

    let result = post_service::create_post(&pool, author_id, dto)
        .await
        .unwrap();

    assert_eq!(result.post.slug, "primeiro-post-do-blog");
    assert_eq!(result.post.title, "Primeiro Post do Blog!");
    assert_eq!(result.post.summary, "Resumo do primeiro post.");
    assert!(result.post.content_html.contains("<h1>Olá Mundo</h1>"));
    assert!(
        result
            .post
            .content_html
            .contains("rel=\"noopener noreferrer\"")
    );
    assert_eq!(result.post.status, PostStatus::Draft);
    assert_eq!(result.post.version, 1);
    assert_eq!(result.tags.len(), 2);
    assert_eq!(result.tags[0].name, "Rust");
    assert_eq!(result.tags[1].name, "Web Development");
}

#[tokio::test]
async fn resolves_slug_collisions_by_appending_numeric_suffix() {
    let (pool, author_id, _dir) = setup_test_db().await;

    let first = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Guia Completo".to_string(),
            summary: None,
            content_md: "Conteúdo 1".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(first.post.slug, "guia-completo");

    let second = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Guia Completo".to_string(),
            summary: None,
            content_md: "Conteúdo 2".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(second.post.slug, "guia-completo-2");

    let third = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Guia Completo!".to_string(),
            summary: None,
            content_md: "Conteúdo 3".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(third.post.slug, "guia-completo-3");
}

#[tokio::test]
async fn validates_title_length_and_empty_publishing_content() {
    let (pool, author_id, _dir) = setup_test_db().await;

    let empty_title_res = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "   ".to_string(),
            summary: None,
            content_md: "Corpo".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await;
    assert!(matches!(
        empty_title_res,
        Err(post_service::PostServiceError::InvalidTitle)
    ));

    let empty_publish_res = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Publicação Vazia".to_string(),
            summary: None,
            content_md: "   ".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: None,
            scheduled_for: None,
        },
    )
    .await;
    assert!(matches!(
        empty_publish_res,
        Err(post_service::PostServiceError::EmptyContentWhenPublished)
    ));
}

#[tokio::test]
async fn updates_post_with_optimistic_concurrency_control() {
    let (pool, author_id, _dir) = setup_test_db().await;

    let created = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Concorrência Otimista".to_string(),
            summary: Some("Resumo inicial".to_string()),
            content_md: "Texto v1".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await
    .unwrap();

    let post_id = created.post.id;
    assert_eq!(created.post.version, 1);

    let conflict_res = post_service::update_post(
        &pool,
        post_id,
        UpdatePostDto {
            title: Some("Tentativa Conflitante".to_string()),
            summary: None,
            content_md: None,
            featured_image_media_id: None,
            status: None,
            tags: None,
            scheduled_for: None,
            version: 99,
        },
    )
    .await;
    assert!(matches!(
        conflict_res,
        Err(post_service::PostServiceError::VersionConflict)
    ));

    let updated = post_service::update_post(
        &pool,
        post_id,
        UpdatePostDto {
            title: Some("Título Atualizado".to_string()),
            summary: Some("Resumo atualizado".to_string()),
            content_md: Some("Novo texto v2 com **estilo**".to_string()),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["Atualizado".to_string()]),
            scheduled_for: None,
            version: 1,
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.post.version, 2);
    assert_eq!(updated.post.title, "Título Atualizado");
    assert!(
        updated
            .post
            .content_html
            .contains("<strong>estilo</strong>")
    );
    assert_eq!(updated.post.status, PostStatus::Published);
    assert!(updated.post.published_at.is_some());
    assert_eq!(updated.tags.len(), 1);
    assert_eq!(updated.tags[0].name, "Atualizado");
}

#[tokio::test]
async fn fts_search_only_returns_published_posts() {
    let (pool, author_id, _dir) = setup_test_db().await;

    post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Rascunho Secreto sobre Rust".to_string(),
            summary: Some("Palavra-chave confidencial".to_string()),
            content_md: "Conteúdo confidencial com termo raro xyz".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await
    .unwrap();

    let draft_search = post_service::search_published_posts(&pool, "xyz", None, None)
        .await
        .unwrap();
    assert_eq!(draft_search.total, 0);
    assert!(draft_search.items.is_empty());

    let published = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Artigo Público sobre Rust e Axum".to_string(),
            summary: Some("Guia de alta performance xyz".to_string()),
            content_md: "Aprenda arquitetura de backend moderna.".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["Rust".to_string(), "Backend".to_string()]),
            scheduled_for: None,
        },
    )
    .await
    .unwrap();

    let search_res = post_service::search_published_posts(&pool, "xyz", None, None)
        .await
        .unwrap();
    assert_eq!(search_res.total, 1);
    assert_eq!(search_res.items[0].id, published.post.id);
    assert_eq!(
        search_res.items[0].title,
        "Artigo Público sobre Rust e Axum"
    );

    let safe_search =
        post_service::search_published_posts(&pool, "\"'()---***;;;xyz***;;;\"", None, None)
            .await
            .unwrap();
    assert_eq!(safe_search.total, 1);

    post_service::delete_post(&pool, published.post.id)
        .await
        .unwrap();
    let after_delete = post_service::search_published_posts(&pool, "xyz", None, None)
        .await
        .unwrap();
    assert_eq!(after_delete.total, 0);
}

#[tokio::test]
async fn rejects_payloads_exceeding_title_summary_and_content_limits() {
    let (pool, author_id, _dir) = setup_test_db().await;

    let long_title = "a".repeat(161);
    let title_res = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: long_title,
            summary: None,
            content_md: "Corpo".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await;
    assert!(matches!(
        title_res,
        Err(post_service::PostServiceError::InvalidTitle)
    ));

    let long_summary = "a".repeat(321);
    let summary_res = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Título Válido".to_string(),
            summary: Some(long_summary),
            content_md: "Corpo".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await;
    assert!(matches!(
        summary_res,
        Err(post_service::PostServiceError::InvalidSummary)
    ));

    let oversized_content = "x".repeat(256 * 1024 + 1);
    let content_res = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Título Válido".to_string(),
            summary: None,
            content_md: oversized_content,
            featured_image_media_id: None,
            status: Some(PostStatus::Draft),
            tags: None,
            scheduled_for: None,
        },
    )
    .await;
    assert!(matches!(
        content_res,
        Err(post_service::PostServiceError::ContentTooLarge)
    ));
}

#[tokio::test]
async fn handles_tag_slug_collisions_and_archive_preservation() {
    let (pool, author_id, _dir) = setup_test_db().await;

    let created = post_service::create_post(
        &pool,
        author_id,
        CreatePostDto {
            title: "Post com Tags Colidentes".to_string(),
            summary: None,
            content_md: "Conteúdo".to_string(),
            featured_image_media_id: None,
            status: Some(PostStatus::Published),
            tags: Some(vec!["Web Dev".to_string(), "web-dev".to_string()]),
            scheduled_for: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(created.tags.len(), 1);
    let published_at = created.post.published_at;
    assert!(published_at.is_some());

    let archived = post_service::update_post(
        &pool,
        created.post.id,
        UpdatePostDto {
            title: None,
            summary: None,
            content_md: None,
            featured_image_media_id: None,
            status: Some(PostStatus::Archived),
            tags: None,
            scheduled_for: None,
            version: created.post.version,
        },
    )
    .await
    .unwrap();

    assert_eq!(archived.post.status, PostStatus::Archived);
    assert_eq!(archived.post.published_at, published_at);
}

#[tokio::test]
async fn fts_search_handles_symbol_only_queries_safely() {
    let (pool, _author_id, _dir) = setup_test_db().await;

    let symbol_res = post_service::search_published_posts(&pool, "@#$%^&*()_+~`", None, None)
        .await
        .unwrap();
    assert_eq!(symbol_res.total, 0);
    assert!(symbol_res.items.is_empty());
}
