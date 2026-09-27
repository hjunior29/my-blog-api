use my_blog_api::{
    auth::password::{self, PasswordError},
    database,
    users::{
        model::{UserRole, UserStatus},
        repository,
        service::{self, SeedResult, UserServiceError},
    },
};

#[test]
fn user_role_and_status_parsing_and_display() {
    assert_eq!(UserRole::Owner.to_string(), "owner");
    assert_eq!(UserRole::Author.to_string(), "author");
    assert_eq!("owner".parse::<UserRole>().unwrap(), UserRole::Owner);
    assert_eq!("author".parse::<UserRole>().unwrap(), UserRole::Author);
    assert!("admin".parse::<UserRole>().is_err());

    assert_eq!(UserStatus::Active.to_string(), "active");
    assert_eq!(UserStatus::Inactive.to_string(), "inactive");
    assert_eq!("active".parse::<UserStatus>().unwrap(), UserStatus::Active);
    assert_eq!(
        "inactive".parse::<UserStatus>().unwrap(),
        UserStatus::Inactive
    );
    assert!("deleted".parse::<UserStatus>().is_err());
}

#[test]
fn email_normalization_rules() {
    assert_eq!(
        service::normalize_email(" Test@Example.Com ").unwrap(),
        "test@example.com"
    );
    assert!(matches!(
        service::normalize_email(""),
        Err(UserServiceError::InvalidEmail)
    ));
    assert!(matches!(
        service::normalize_email("noatsign.com"),
        Err(UserServiceError::InvalidEmail)
    ));
    assert!(matches!(
        service::normalize_email("@nodomain"),
        Err(UserServiceError::InvalidEmail)
    ));
    assert!(matches!(
        service::normalize_email("user@nodot"),
        Err(UserServiceError::InvalidEmail)
    ));
    assert!(matches!(
        service::normalize_email("user@exämple.com"),
        Err(UserServiceError::InvalidEmail)
    ));
}

#[tokio::test]
async fn seed_owner_is_idempotent_and_enforces_single_owner() {
    let pool = database::connect("sqlite::memory:", 2).await.unwrap();

    let res1 = service::seed_owner(
        &pool,
        "Owner@domain.com",
        "Main Owner",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();
    let id1 = match res1 {
        SeedResult::Created(id) => id,
        _ => panic!("Expected SeedResult::Created"),
    };

    let res2 = service::seed_owner(
        &pool,
        "owner@domain.com",
        "Main Owner",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();
    assert_eq!(res2, SeedResult::AlreadyExists(id1));

    let res3 = service::seed_owner(
        &pool,
        "other@domain.com",
        "Other Owner",
        "correct horse battery staple 2026",
    )
    .await;
    assert!(matches!(res3, Err(UserServiceError::OwnerAlreadyExists)));
    pool.close().await;
}

#[tokio::test]
async fn last_owner_protection_prevents_disabling_or_demoting_only_owner() {
    let pool = database::connect("sqlite::memory:", 2).await.unwrap();
    let res = service::seed_owner(
        &pool,
        "admin@domain.com",
        "Admin",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();
    let owner_id = match res {
        SeedResult::Created(id) => id,
        _ => unreachable!(),
    };

    let disable_err = service::disable_user(&pool, owner_id).await;
    assert!(matches!(
        disable_err,
        Err(UserServiceError::LastOwnerProtection)
    ));

    let demote_err = service::set_user_role(&pool, owner_id, UserRole::Author).await;
    assert!(matches!(
        demote_err,
        Err(UserServiceError::LastOwnerProtection)
    ));
    pool.close().await;
}

#[tokio::test]
async fn password_reset_validates_policy_and_updates_hash() {
    let pool = database::connect("sqlite::memory:", 2).await.unwrap();
    let res = service::seed_owner(
        &pool,
        "admin@domain.com",
        "Admin",
        "correct horse battery staple 2026",
    )
    .await
    .unwrap();
    let user_id = match res {
        SeedResult::Created(id) => id,
        _ => unreachable!(),
    };

    let reset_err = service::reset_password(&pool, user_id, "short").await;
    assert!(matches!(
        reset_err,
        Err(UserServiceError::Password(PasswordError::InvalidPolicy))
    ));

    let new_pass = "super secret valid password 2026";
    service::reset_password(&pool, user_id, new_pass)
        .await
        .unwrap();

    let updated = repository::find_by_id(&pool, user_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        password::verify_password(new_pass.to_string(), updated.password_hash)
            .await
            .unwrap()
    );
    pool.close().await;
}

#[tokio::test]
async fn concurrent_seed_owner_calls_do_not_create_duplicate_owners() {
    let pool = database::connect("sqlite::memory:", 4).await.unwrap();
    let pool1 = pool.clone();
    let pool2 = pool.clone();

    let t1 = tokio::spawn(async move {
        service::seed_owner(
            &pool1,
            "admin@domain.com",
            "Admin 1",
            "correct horse battery staple 2026",
        )
        .await
    });
    let t2 = tokio::spawn(async move {
        service::seed_owner(
            &pool2,
            "admin@domain.com",
            "Admin 2",
            "correct horse battery staple 2026",
        )
        .await
    });

    let (r1, r2) = tokio::join!(t1, t2);
    let r1 = r1.unwrap();
    let r2 = r2.unwrap();

    let created_count = [r1.is_ok(), r2.is_ok()].into_iter().filter(|&x| x).count();
    assert!(created_count >= 1);

    let owners_count = repository::count_active_owners(&pool).await.unwrap();
    assert_eq!(owners_count, 1);
    pool.close().await;
}

#[tokio::test]
async fn create_author_user_via_service_succeeds_and_prevents_duplicate_email() {
    let pool = database::connect("sqlite::memory:", 2).await.unwrap();

    let user_id = service::create_user(
        &pool,
        "author@example.com",
        "Author Name",
        "valid author password 12345",
        UserRole::Author,
    )
    .await
    .unwrap();

    let user = repository::find_by_id(&pool, user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.email, "author@example.com");
    assert_eq!(user.role, UserRole::Author);
    assert_eq!(user.status, UserStatus::Active);

    let dup_err = service::create_user(
        &pool,
        "AUTHOR@example.com",
        "Another Author",
        "valid author password 12345",
        UserRole::Author,
    )
    .await
    .unwrap_err();

    assert!(matches!(dup_err, UserServiceError::EmailAlreadyRegistered));
    pool.close().await;
}
