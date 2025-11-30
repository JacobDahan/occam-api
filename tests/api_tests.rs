use axum_test::TestServer;
use serde_json::json;

use occam_api::api::{create_router, AppState};

fn create_test_server() -> TestServer {
    let state = AppState::new();
    let app = create_router(state);
    TestServer::new(app).unwrap()
}

#[tokio::test]
async fn test_health_check() {
    let server = create_test_server();
    let response = server.get("/health").await;
    response.assert_status_ok();
}

#[tokio::test]
async fn test_create_and_get_service() {
    let server = create_test_server();

    // Create a service
    let response = server
        .post("/services")
        .json(&json!({
            "name": "Netflix",
            "monthly_cost_cents": 1599
        }))
        .await;

    response.assert_status(axum::http::StatusCode::CREATED);
    let created: serde_json::Value = response.json();
    assert_eq!(created["name"], "Netflix");
    assert_eq!(created["monthly_cost_cents"], 1599);

    // Get all services
    let response = server.get("/services").await;
    response.assert_status_ok();
    let services: Vec<serde_json::Value> = response.json();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0]["name"], "Netflix");
}

#[tokio::test]
async fn test_create_and_get_title() {
    let server = create_test_server();

    // Create a title
    let response = server
        .post("/titles")
        .json(&json!({
            "name": "The Matrix",
            "content_type": "movie"
        }))
        .await;

    response.assert_status(axum::http::StatusCode::CREATED);
    let created: serde_json::Value = response.json();
    assert_eq!(created["name"], "The Matrix");
    assert_eq!(created["content_type"], "movie");

    // Get all titles
    let response = server.get("/titles").await;
    response.assert_status_ok();
    let titles: Vec<serde_json::Value> = response.json();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0]["name"], "The Matrix");
}

#[tokio::test]
async fn test_add_title_preference() {
    let server = create_test_server();

    // First create a title
    let response = server
        .post("/titles")
        .json(&json!({
            "name": "Breaking Bad",
            "content_type": "tv_show"
        }))
        .await;
    let title: serde_json::Value = response.json();
    let title_id = title["id"].as_str().unwrap();

    // Add as must have
    let response = server
        .post("/preferences/titles")
        .json(&json!({
            "title_id": title_id,
            "priority": "must_have"
        }))
        .await;
    response.assert_status_ok();

    // Verify preferences
    let response = server.get("/preferences").await;
    response.assert_status_ok();
    let prefs: serde_json::Value = response.json();
    assert_eq!(prefs["titles"].as_array().unwrap().len(), 1);
    assert_eq!(prefs["titles"][0]["priority"], "must_have");
}

#[tokio::test]
async fn test_optimization_flow() {
    let server = create_test_server();

    // Create titles
    let title1_resp = server
        .post("/titles")
        .json(&json!({
            "name": "The Matrix",
            "content_type": "movie"
        }))
        .await;
    let title1: serde_json::Value = title1_resp.json();
    let title1_id = title1["id"].as_str().unwrap();

    let title2_resp = server
        .post("/titles")
        .json(&json!({
            "name": "Breaking Bad",
            "content_type": "tv_show"
        }))
        .await;
    let title2: serde_json::Value = title2_resp.json();
    let title2_id = title2["id"].as_str().unwrap();

    // Create services with titles
    let service1_resp = server
        .post("/services")
        .json(&json!({
            "name": "Netflix",
            "monthly_cost_cents": 1599,
            "available_titles": [title1_id, title2_id]
        }))
        .await;
    let _: serde_json::Value = service1_resp.json();

    let _service2_resp = server
        .post("/services")
        .json(&json!({
            "name": "Hulu",
            "monthly_cost_cents": 999,
            "available_titles": [title1_id]
        }))
        .await;

    // Add title preferences
    server
        .post("/preferences/titles")
        .json(&json!({
            "title_id": title1_id,
            "priority": "must_have"
        }))
        .await;

    server
        .post("/preferences/titles")
        .json(&json!({
            "title_id": title2_id,
            "priority": "nice_to_have"
        }))
        .await;

    // Run optimization
    let response = server.get("/optimize").await;
    response.assert_status_ok();

    let result: serde_json::Value = response.json();
    
    // Should recommend Netflix since it covers both titles
    assert!(result["recommended_services"].as_array().unwrap().len() >= 1);
    assert!(result["must_have_covered"].as_array().unwrap().len() >= 1);
}

#[tokio::test]
async fn test_optimization_with_no_services() {
    let server = create_test_server();

    // Try to optimize without any services
    let response = server.get("/optimize").await;
    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}
