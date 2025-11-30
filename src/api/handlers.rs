use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{ContentType, Priority, StreamingService, Title};
use crate::services::Optimizer;

use super::AppState;

// Request/Response types

#[derive(Debug, Deserialize)]
pub struct CreateServiceRequest {
    pub name: String,
    pub monthly_cost_cents: u32,
    pub available_titles: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct ServiceResponse {
    pub id: Uuid,
    pub name: String,
    pub monthly_cost_cents: u32,
    pub available_titles: Vec<Uuid>,
}

impl From<&StreamingService> for ServiceResponse {
    fn from(service: &StreamingService) -> Self {
        Self {
            id: service.id,
            name: service.name.clone(),
            monthly_cost_cents: service.monthly_cost_cents,
            available_titles: service.available_titles.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateTitleRequest {
    pub name: String,
    pub content_type: ContentType,
}

#[derive(Debug, Serialize)]
pub struct TitleResponse {
    pub id: Uuid,
    pub name: String,
    pub content_type: ContentType,
}

impl From<&Title> for TitleResponse {
    fn from(title: &Title) -> Self {
        Self {
            id: title.id,
            name: title.name.clone(),
            content_type: title.content_type.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AddTitlePreferenceRequest {
    pub title_id: Uuid,
    pub priority: Priority,
}

#[derive(Debug, Deserialize)]
pub struct AddSubscriptionRequest {
    pub service_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct OptimizeResponse {
    pub recommended_services: Vec<ServiceResponse>,
    pub total_monthly_cost_cents: u32,
    pub must_have_covered: Vec<TitleResponse>,
    pub nice_to_have_covered: Vec<TitleResponse>,
    pub unavailable_titles: Vec<Uuid>,
}

// Handlers

/// Health check endpoint
pub async fn health_check() -> StatusCode {
    StatusCode::OK
}

/// Get all streaming services
pub async fn get_services(
    State(state): State<AppState>,
) -> Json<Vec<ServiceResponse>> {
    let inner = state.inner.read().await;
    let services: Vec<ServiceResponse> = inner.services.values().map(ServiceResponse::from).collect();
    Json(services)
}

/// Create a new streaming service
pub async fn create_service(
    State(state): State<AppState>,
    Json(request): Json<CreateServiceRequest>,
) -> (StatusCode, Json<ServiceResponse>) {
    let mut service = StreamingService::new(request.name, request.monthly_cost_cents);
    
    if let Some(titles) = request.available_titles {
        for title_id in titles {
            service.add_title(title_id);
        }
    }

    let response = ServiceResponse::from(&service);
    
    let mut inner = state.inner.write().await;
    inner.services.insert(service.id, service);

    (StatusCode::CREATED, Json(response))
}

/// Get all titles
pub async fn get_titles(
    State(state): State<AppState>,
) -> Json<Vec<TitleResponse>> {
    let inner = state.inner.read().await;
    let titles: Vec<TitleResponse> = inner.titles.values().map(TitleResponse::from).collect();
    Json(titles)
}

/// Create a new title
pub async fn create_title(
    State(state): State<AppState>,
    Json(request): Json<CreateTitleRequest>,
) -> (StatusCode, Json<TitleResponse>) {
    let title = Title::new(request.name, request.content_type);
    let response = TitleResponse::from(&title);
    
    let mut inner = state.inner.write().await;
    inner.titles.insert(title.id, title);

    (StatusCode::CREATED, Json(response))
}

/// Add a title preference
pub async fn add_title_preference(
    State(state): State<AppState>,
    Json(request): Json<AddTitlePreferenceRequest>,
) -> StatusCode {
    let mut inner = state.inner.write().await;
    inner.user_preferences.add_title(request.title_id, request.priority);
    StatusCode::OK
}

/// Add a current subscription
pub async fn add_subscription(
    State(state): State<AppState>,
    Json(request): Json<AddSubscriptionRequest>,
) -> StatusCode {
    let mut inner = state.inner.write().await;
    inner.user_preferences.add_subscription(request.service_id);
    StatusCode::OK
}

/// Get user preferences
pub async fn get_preferences(
    State(state): State<AppState>,
) -> Json<crate::models::UserPreferences> {
    let inner = state.inner.read().await;
    Json(inner.user_preferences.clone())
}

/// Run optimization to find best streaming service subset
pub async fn optimize(
    State(state): State<AppState>,
) -> Result<Json<OptimizeResponse>, (StatusCode, String)> {
    let inner = state.inner.read().await;
    
    let services: Vec<StreamingService> = inner.services.values().cloned().collect();
    
    if services.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No streaming services available".to_string()));
    }

    let optimizer = Optimizer::new(&services, &inner.user_preferences);
    
    match optimizer.optimize() {
        Ok(result) => {
            let recommended_services: Vec<ServiceResponse> = result.recommended_services
                .iter()
                .filter_map(|id| inner.services.get(id))
                .map(ServiceResponse::from)
                .collect();

            let must_have_covered: Vec<TitleResponse> = result.must_have_covered
                .iter()
                .filter_map(|id| inner.titles.get(id))
                .map(TitleResponse::from)
                .collect();

            let nice_to_have_covered: Vec<TitleResponse> = result.nice_to_have_covered
                .iter()
                .filter_map(|id| inner.titles.get(id))
                .map(TitleResponse::from)
                .collect();

            Ok(Json(OptimizeResponse {
                recommended_services,
                total_monthly_cost_cents: result.total_monthly_cost_cents,
                must_have_covered,
                nice_to_have_covered,
                unavailable_titles: result.unavailable_titles,
            }))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}
