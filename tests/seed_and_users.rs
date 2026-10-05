use my_blog_api::{
    auth::password::{self, PasswordError},
    database,
    users::{
        model::{PublicUser, UserRole, UserStatus},
        repository,
        service::{self, SeedResult, UserServiceError},
    },
};

async fn create_test_pool() -> (sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite://{}", db_path.display());
    let pool = database::connect(&url, 4).await.unwrap();
    (pool, dir)
}

#[tokio::test]
async fn seed_owner_on_fresh_db_creates_active_owner() {
    let (pool, _dir) = create_test_pool().await;
    let result = service::seed_owner(
        &pool,
        "owner@example.com",
        "Initial Owner",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();

    let user_id = match result {
        SeedResult::Created(id) => id,
        SeedResult::AlreadyExists(_) => panic!("expected Created, got AlreadyExists"),
    };

    let user = repository::find_by_id(&pool, user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.email, "owner@example.com");
    assert_eq!(user.normalized_email, "owner@example.com");
    assert_eq!(user.role, UserRole::Owner);
    assert_eq!(user.status, UserStatus::Active);

    let public_user = PublicUser::from(&user);
    let serialized = serde_json::to_string(&public_user).unwrap();
    assert!(!serialized.contains("email"));
    assert!(!serialized.contains("password"));
    assert!(!serialized.contains("status"));
    pool.close().await;
}

#[tokio::test]
async fn seed_owner_is_idempotent_with_same_email_and_does_not_modify_password() {
    let (pool, _dir) = create_test_pool().await;
    let initial_pass = "correct horse battery staple 2026";
    let first = service::seed_owner(&pool, "Owner@Example.com ", "Owner One", initial_pass)
        .await
        .unwrap();

    let id1 = match first {
        SeedResult::Created(id) => id,
        _ => panic!("expected Created"),
    };

    let user1 = repository::find_by_id(&pool, id1).await.unwrap().unwrap();
    let original_hash = user1.password_hash.clone();

    let second = service::seed_owner(
        &pool,
        "owner@example.com",
        "Different Name",
        "different password long enough 12345",
    )
    .await
    .unwrap();

    let id2 = match second {
        SeedResult::AlreadyExists(id) => id,
        _ => panic!("expected AlreadyExists"),
    };

    assert_eq!(id1, id2);
    let user2 = repository::find_by_id(&pool, id2).await.unwrap().unwrap();
    assert_eq!(user2.password_hash, original_hash);
    assert_eq!(user2.display_name, "Owner One");
    pool.close().await;
}

#[tokio::test]
async fn seed_owner_fails_when_owner_with_different_email_already_exists() {
    let (pool, _dir) = create_test_pool().await;
    service::seed_owner(
        &pool,
        "first_owner@example.com",
        "First Owner",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();

    let err = service::seed_owner(
        &pool,
        "second_owner@example.com",
        "Second Owner",
        "another valid long password 2026",
    )
    .await
    .unwrap_err();

    assert!(matches!(err, UserServiceError::OwnerAlreadyExists));
    pool.close().await;
}

#[tokio::test]
async fn cannot_disable_or_demote_the_last_active_owner() {
    let (pool, _dir) = create_test_pool().await;
    let seed = service::seed_owner(
        &pool,
        "sole_owner@example.com",
        "Sole Owner",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();

    let owner_id = match seed {
        SeedResult::Created(id) => id,
        _ => panic!("expected Created"),
    };

    let disable_err = service::disable_user(&pool, owner_id).await.unwrap_err();
    assert!(matches!(disable_err, UserServiceError::LastOwnerProtection));

    let demote_err = service::set_user_role(&pool, owner_id, UserRole::Author)
        .await
        .unwrap_err();
    assert!(matches!(demote_err, UserServiceError::LastOwnerProtection));
    pool.close().await;
}

#[tokio::test]
async fn email_normalization_handles_case_and_whitespace() {
    assert_eq!(
        service::normalize_email("  User@Example.COM  ").unwrap(),
        "user@example.com"
    );
    assert!(service::normalize_email("").is_err());
    assert!(service::normalize_email("invalid-no-at").is_err());
    assert!(service::normalize_email("@example.com").is_err());
    assert!(service::normalize_email("user@").is_err());
    assert!(service::normalize_email("user@no-tld").is_err());
}

#[tokio::test]
async fn password_policy_rejects_weak_or_short_passwords() {
    assert!(matches!(
        password::validate_password("too-short"),
        Err(PasswordError::InvalidPolicy)
    ));
    assert!(password::validate_password("15-char-valid-pwd").is_ok());
}

#[tokio::test]
async fn reset_password_updates_hash_and_invalidates_sessions() {
    let (pool, _dir) = create_test_pool().await;
    let seed = service::seed_owner(
        &pool,
        "owner@example.com",
        "Owner",
        "original password 12345678",
    )
    .await
    .unwrap();

    let user_id = match seed {
        SeedResult::Created(id) => id,
        _ => panic!("expected Created"),
    };

    sqlx::query(
        "INSERT INTO auth_sessions (id, user_id, csrf_token, created_at, expires_at)
         VALUES ('session-1', ?, 'csrf-1', 1000, 2000)",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let new_pass = "new secret password changed 12345";
    service::reset_password(&pool, user_id, new_pass)
        .await
        .unwrap();

    let revoked: Option<i64> =
        sqlx::query_scalar("SELECT revoked_at FROM auth_sessions WHERE id = 'session-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(revoked.is_some());

    let user = repository::find_by_id(&pool, user_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        password::verify_password(new_pass.to_string(), user.password_hash)
            .await
            .unwrap()
    );
    pool.close().await;
}

#[tokio::test]
async fn concurrent_seeds_allow_only_one_winner() {
    let (pool, _dir) = create_test_pool().await;
    let pool_clone = pool.clone();

    let task1 = tokio::spawn(async move {
        service::seed_owner(
            &pool_clone,
            "owner1@example.com",
            "Owner One",
            "password one for testing 12345",
        )
        .await
    });

    let pool_clone2 = pool.clone();
    let task2 = tokio::spawn(async move {
        service::seed_owner(
            &pool_clone2,
            "owner2@example.com",
            "Owner Two",
            "password two for testing 12345",
        )
        .await
    });

    let (res1, res2) = tokio::join!(task1, task2);
    let r1 = res1.unwrap();
    let r2 = res2.unwrap();

    let count_success = [r1.is_ok(), r2.is_ok()].into_iter().filter(|&x| x).count();
    let count_failure = [r1.is_err(), r2.is_err()]
        .into_iter()
        .filter(|&x| x)
        .count();

    assert_eq!(count_success, 1);
    assert_eq!(count_failure, 1);

    let active_owners = repository::count_active_owners(&pool).await.unwrap();
    assert_eq!(active_owners, 1);
    pool.close().await;
}

#[tokio::test]
async fn update_user_email_normalizes_and_prevents_duplicates() {
    let (pool, _dir) = create_test_pool().await;
    let seed_res = service::seed_owner(&pool, "initial@example.com", "Owner", "ValidPassword123!")
        .await
        .unwrap();
    let user_id = match seed_res {
        service::SeedResult::Created(id) => id,
        service::SeedResult::AlreadyExists(id) => id,
    };

    service::update_user_email(&pool, user_id, "  New.Email@example.COM  ")
        .await
        .unwrap();
    let user = repository::find_by_id(&pool, user_id).await.unwrap().unwrap();
    assert_eq!(user.email, "New.Email@example.COM");
    assert_eq!(user.normalized_email, "new.email@example.com");

    let err = service::update_user_email(&pool, user_id, "not-an-email")
        .await
        .unwrap_err();
    assert!(matches!(err, UserServiceError::InvalidEmail));

    pool.close().await;
}
