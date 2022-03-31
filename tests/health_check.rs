use sqlx::{PgPool, PgConnection, Connection, Executor};
use uuid::Uuid;
use std::net::TcpListener;
use zero2prod::{configuration::{get_configuration, DatabaseSettings}, startup::run};

struct TestApp {
    address: String,
    db_pool: PgPool,
}

async fn spwan_app() -> TestApp {
    let listner = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listner.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{port}");

    let mut configuration = get_configuration().expect("Failed to read configuration.");
    configuration.database.database_name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(listner, connection_pool.clone()).expect("Failed to bind address");
    let _ = tokio::spawn(server);
    TestApp {
        address,
        db_pool: connection_pool,
    }
}

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
    let mut connection = PgConnection::connect(&config.connection_string_without_db()).await.expect("Failed to connect to Postgres");
    connection.execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str()).await.expect("Failed to create database.");

    let connection_pool = PgPool::connect(&config.connection_string()).await.expect("Failed to connect to Postgres.");

    sqlx::migrate!("./migrations").run(&connection_pool).await.expect("Failed to migrate the database");

    connection_pool
}

#[tokio::test]
async fn health_check_works() {
    let app = spwan_app().await;
    let client = reqwest::Client::new();

    let response = client
        .get(app.address + "/health_check")
        .send()
        .await
        .expect("Failed to execute request.");

    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

#[tokio::test]
async fn subscribe_returns_a_200_for_valid_form_data() {
    let app = spwan_app().await;
    let client = reqwest::Client::new();

    let body = "name=Swanand%20Mulay&email=swanandx%40github.com";
    let response = client
        .post(app.address + "/subscriptions")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .expect("Failed to execute reqwest.");

    assert_eq!(200, response.status().as_u16());

    let saved = sqlx::query!("SELECT email, name FROM subscriptions",)
        .fetch_one(&app.db_pool)
        .await
        .expect("Failed to fetch saved subscriptionn.");

    assert_eq!(saved.email, "swanandx@github.com");
    assert_eq!(saved.name, "Swanand Mulay");
}

#[tokio::test]
async fn subscribe_returns_a_400_when_data_is_missing() {
    let app = spwan_app().await;
    let client = reqwest::Client::new();
    let test_cases = vec![
        ("name=swanand%20mulay", "missing the email"),
        ("email=swanandx@github.com", "missing the name"),
        ("", "missing both name and email"),
    ];

    for (invalid_body, error_message) in test_cases {
        let response = client
            .post(format!("{}/subscriptions", &app.address))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(invalid_body)
            .send()
            .await
            .expect("Failed to execute reqwest.");

        assert_eq!(
            400,
            response.status().as_u16(),
            "The API did not fail with 400 Bad Request when the payload was {error_message}."
        );
    }
}
