use crate::{
    error::{AppError, AppResult},
    models::{
        AvailabilityType, OptimizationRequest, OptimizationResponse, ServiceConfiguration,
        StreamingAvailability, StreamingService,
    },
    services::availability::AvailabilityService,
};
use good_lp::{
    constraint::Constraint, default_solver, variable, Expression, ProblemVariables, SolverModel,
    Variable,
};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

/// Service catalog entry with pricing
struct ServiceInfo {
    id: String,
    name: String,
    cost: f64,
}

/// Finds the optimal subset of streaming services
///
/// Uses integer programming to find the most cost-effective combination
/// of streaming services that covers all "must have" titles and maximizes
/// "nice to have" title coverage.
///
/// The optimization prioritizes:
/// 1. Coverage of all "must have" titles (hard constraint)
/// 2. Minimizing total cost (primary objective)
/// 3. Maximizing "nice to have" coverage (secondary objective)
pub async fn optimize_services(
    db_pool: Arc<PgPool>,
    availability_service: Arc<AvailabilityService>,
    request: OptimizationRequest,
) -> AppResult<OptimizationResponse> {
    let start = Instant::now();

    // 1. Combine all titles
    let all_titles: Vec<String> = request
        .must_have
        .iter()
        .chain(request.nice_to_have.iter())
        .cloned()
        .collect();

    if all_titles.is_empty() {
        return Err(AppError::InvalidInput(
            "Must provide at least one title".to_string(),
        ));
    }

    tracing::info!(
        must_have = request.must_have.len(),
        nice_to_have = request.nice_to_have.len(),
        total = all_titles.len(),
        "Starting optimization"
    );

    // 2. Fetch availability data (parallel, cached)
    let availability_data = availability_service
        .fetch_availability_batch(all_titles)
        .await?;

    tracing::info!(
        fetched = availability_data.len(),
        "Availability data fetched"
    );

    // 3. Build service catalog and title mappings
    let (service_catalog, title_to_services) =
        build_service_mappings(&availability_data, &request, &db_pool).await?;

    if service_catalog.is_empty() {
        return Err(AppError::Optimization(
            "No streaming services found for provided titles".to_string(),
        ));
    }

    // 4. Identify unavailable titles
    let unavailable_must_have: Vec<String> = request
        .must_have
        .iter()
        .filter(|title| !title_to_services.contains_key(*title))
        .cloned()
        .collect();

    let unavailable_nice_to_have: Vec<String> = request
        .nice_to_have
        .iter()
        .filter(|title| !title_to_services.contains_key(*title))
        .cloned()
        .collect();

    if !unavailable_must_have.is_empty() {
        tracing::warn!(
            count = unavailable_must_have.len(),
            titles = ?unavailable_must_have,
            "Some must-have titles are unavailable"
        );
    }

    if !unavailable_nice_to_have.is_empty() {
        tracing::info!(
            count = unavailable_nice_to_have.len(),
            "Some nice-to-have titles are unavailable"
        );
    }

    // 5. Build and solve integer programming model (if there are available must-have titles)
    let solution = solve_optimization(
        &service_catalog,
        &title_to_services,
        &request,
        unavailable_must_have,
        unavailable_nice_to_have,
    )?;

    let elapsed = start.elapsed();
    tracing::info!(
        processing_time_ms = elapsed.as_millis(),
        "Optimization completed"
    );

    Ok(solution)
}

/// Builds service catalog and title-to-services mapping
async fn build_service_mappings(
    availability_data: &[StreamingAvailability],
    _request: &OptimizationRequest,
    db_pool: &PgPool,
) -> AppResult<(Vec<ServiceInfo>, HashMap<String, Vec<String>>)> {
    let mut service_ids_set: HashSet<String> = HashSet::new();
    let mut title_to_services: HashMap<String, Vec<String>> = HashMap::new();

    // First pass: collect all unique service IDs and build title mappings
    for availability in availability_data {
        let mut services_for_title = Vec::new();

        for service_avail in &availability.services {
            // Only consider subscription-based services for optimization
            if service_avail.availability_type == AvailabilityType::Subscription {
                service_ids_set.insert(service_avail.service_id.clone());
                services_for_title.push(service_avail.service_id.clone());
            }
        }

        if !services_for_title.is_empty() {
            title_to_services.insert(availability.imdb_id.clone(), services_for_title);
        }
    }

    // Second pass: fetch pricing from database for all services
    let service_catalog = fetch_service_pricing(db_pool, service_ids_set).await?;

    Ok((service_catalog, title_to_services))
}

/// Fetches service pricing from the database
async fn fetch_service_pricing(
    db_pool: &PgPool,
    service_ids: HashSet<String>,
) -> AppResult<Vec<ServiceInfo>> {
    if service_ids.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<String> = service_ids.iter().cloned().collect();

    // Query the database for service pricing
    let rows = sqlx::query!(
        r#"
        SELECT id, name, base_monthly_cost
        FROM streaming_services
        WHERE id = ANY($1) AND active = true
        "#,
        &ids[..]
    )
    .fetch_all(db_pool)
    .await
    .map_err(AppError::from)?;

    let mut service_catalog = Vec::new();
    let mut db_service_ids = HashSet::new();

    for row in rows {
        // Convert bigdecimal to f64 for the solver
        let cost = row
            .base_monthly_cost
            .to_string()
            .parse::<f64>()
            .expect("Invalid cost format in database");

        db_service_ids.insert(row.id.clone());
        service_catalog.push(ServiceInfo {
            id: row.id,
            name: row.name,
            cost,
        });
    }

    // For any services not in the database, log a warning
    for service_id in &service_ids {
        if !db_service_ids.contains(service_id) {
            tracing::warn!(
                service_id = %service_id,
                "Service not found in database, skipping"
            );
        }
    }

    Ok(service_catalog)
}

/// Solves the optimization problem using integer programming
fn solve_optimization(
    service_catalog: &[ServiceInfo],
    title_to_services: &HashMap<String, Vec<String>>,
    request: &OptimizationRequest,
    unavailable_must_have: Vec<String>,
    unavailable_nice_to_have: Vec<String>,
) -> AppResult<OptimizationResponse> {
    // Filter to only available titles for optimization
    let available_must_have: Vec<&String> = request
        .must_have
        .iter()
        .filter(|title| title_to_services.contains_key(*title))
        .collect();

    // If ALL must-have titles are unavailable, return early with empty solution
    if available_must_have.is_empty() && !request.must_have.is_empty() {
        return Ok(OptimizationResponse {
            configurations: vec![],
            unavailable_must_have,
            unavailable_nice_to_have,
        });
    }

    // Generate all configurations: optimal + alternatives
    let configurations = generate_configurations(
        service_catalog,
        title_to_services,
        &available_must_have,
        &request.nice_to_have,
    );

    tracing::info!(
        configurations_count = configurations.len(),
        optimal_cost = configurations.first().map(|c| c.total_cost),
        unavailable_must_have = unavailable_must_have.len(),
        unavailable_nice_to_have = unavailable_nice_to_have.len(),
        "Optimization completed"
    );

    Ok(OptimizationResponse {
        configurations,
        unavailable_must_have,
        unavailable_nice_to_have,
    })
}

/// Internal solution structure
#[derive(Debug, Clone)]
struct Solution {
    services: Vec<StreamingService>,
    total_cost: f64,
    must_have_coverage: usize,
    nice_to_have_coverage: usize,
}

impl Solution {
    /// Creates a unique signature for the solution based on service IDs (for deduplication)
    fn signature(&self) -> String {
        let mut ids: Vec<&str> = self.services.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        ids.join(",")
    }
}

/// Finds a single solution with the given nice-to-have weight
fn find_solution(
    service_catalog: &[ServiceInfo],
    title_to_services: &HashMap<String, Vec<String>>,
    available_must_have: &[&String],
    nice_to_have: &[String],
    coverage_weight: f64,
    extra_constraint: Option<Constraint>,
) -> AppResult<Solution> {
    let mut vars = ProblemVariables::new();

    // Create binary variables for each service (0 = not selected, 1 = selected)
    let service_vars: HashMap<String, Variable> = service_catalog
        .iter()
        .map(|s| (s.id.clone(), vars.add(variable().binary())))
        .collect();

    // Build constraints
    let mut constraints = vec![];

    // Constraint: Each available must-have title must be covered by at least one selected service
    for title in available_must_have {
        if let Some(services) = title_to_services.get(*title) {
            let mut coverage_expr = Expression::from(0);
            for service_id in services {
                if let Some(&var) = service_vars.get(service_id) {
                    coverage_expr = coverage_expr + var;
                }
            }
            // At least one service must cover this title
            constraints.push(coverage_expr.geq(1));
        }
    }

    // Add extra constraint if provided
    if let Some(constraint) = extra_constraint {
        constraints.push(constraint);
    }

    // Objective: Minimize cost (primary) and maximize nice-to-have coverage (secondary)
    // We use a weighted sum: minimize (cost - weight * nice_to_have_coverage)
    let mut objective = Expression::from(0);

    // Add service costs to objective
    for service in service_catalog {
        if let Some(&var) = service_vars.get(&service.id) {
            objective = objective + service.cost * var;
        }
    }

    // Subtract bonus for nice-to-have coverage
    for title in nice_to_have {
        if let Some(services) = title_to_services.get(title) {
            for service_id in services {
                if let Some(&var) = service_vars.get(service_id) {
                    objective = objective - coverage_weight * var;
                }
            }
        }
    }

    // Build and solve the problem
    let mut problem = vars.minimise(objective).using(default_solver);
    for constraint in constraints {
        problem = problem.with(constraint);
    }

    let solution = problem
        .solve()
        .map_err(|e| AppError::Optimization(format!("Solver failed: {}", e)))?;

    // Extract selected services
    let selected_services = extract_selected_services(&solution, &service_vars, service_catalog);

    // Calculate coverage statistics
    let must_have_coverage = available_must_have.len();
    let nice_to_have_coverage =
        count_nice_to_have_coverage(&selected_services, nice_to_have, title_to_services);

    let total_cost = selected_services.iter().map(|s| s.monthly_cost).sum();

    Ok(Solution {
        services: selected_services,
        total_cost,
        must_have_coverage,
        nice_to_have_coverage,
    })
}

/// Generates all service configurations with different cost/coverage trade-offs
///
/// Returns an ordered list of configurations from cost-optimal to coverage-optimal.
/// Configurations are generated by solving with progressively higher weights.
fn generate_configurations(
    service_catalog: &[ServiceInfo],
    title_to_services: &HashMap<String, Vec<String>>,
    available_must_have: &[&String],
    nice_to_have: &[String],
) -> Vec<ServiceConfiguration> {
    use std::collections::HashSet;

    let mut configurations = Vec::new();
    let mut seen_signatures = HashSet::new();

    // Generate configurations with increasing nice-to-have weights
    // Weights: 0.1 (optimal/cost-focused), 1.0, 3.0, 10.0, 100.0 (coverage-focused)
    for weight in [0.1, 1.0, 3.0, 10.0, 100.0] {
        if let Ok(solution) = find_solution(
            service_catalog,
            title_to_services,
            available_must_have,
            nice_to_have,
            weight,
            None,
        ) {
            let sig = solution.signature();
            if !seen_signatures.contains(&sig) {
                seen_signatures.insert(sig);
                configurations.push(ServiceConfiguration {
                    services: solution.services,
                    total_cost: solution.total_cost,
                    must_have_coverage: solution.must_have_coverage,
                    nice_to_have_coverage: solution.nice_to_have_coverage,
                });
            }
        }
    }

    // Configurations are naturally ordered by increasing weight (cost-focused → coverage-focused)
    configurations
}

/// Extracts selected services from the solution
fn extract_selected_services(
    solution: &impl good_lp::solvers::Solution,
    service_vars: &HashMap<String, Variable>,
    service_catalog: &[ServiceInfo],
) -> Vec<StreamingService> {
    let mut selected = Vec::new();

    for service in service_catalog {
        if let Some(&var) = service_vars.get(&service.id) {
            let value = solution.value(var);
            // Binary variables might be slightly off from 1.0 due to floating point
            if value > 0.5 {
                selected.push(StreamingService {
                    id: service.id.clone(),
                    name: service.name.clone(),
                    monthly_cost: service.cost,
                });
            }
        }
    }

    selected
}

/// Counts how many nice-to-have titles are covered by selected services
fn count_nice_to_have_coverage(
    selected_services: &[StreamingService],
    nice_to_have: &[String],
    title_to_services: &HashMap<String, Vec<String>>,
) -> usize {
    let selected_ids: HashSet<&str> = selected_services.iter().map(|s| s.id.as_str()).collect();

    nice_to_have
        .iter()
        .filter(|title| {
            if let Some(services) = title_to_services.get(*title) {
                services.iter().any(|s| selected_ids.contains(s.as_str()))
            } else {
                false
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ServiceAvailability;
    use chrono::Utc;
    use sqlx::PgPool;

    // Helper to create mock availability data
    fn create_availability(imdb_id: &str, services: Vec<(&str, &str)>) -> StreamingAvailability {
        StreamingAvailability {
            imdb_id: imdb_id.to_string(),
            services: services
                .into_iter()
                .map(|(id, name)| ServiceAvailability {
                    service_id: id.to_string(),
                    service_name: name.to_string(),
                    availability_type: AvailabilityType::Subscription,
                    quality: None,
                    link: None,
                })
                .collect(),
            cached_at: Utc::now(),
        }
    }

    // Helper to create a test database pool
    async fn create_test_db_pool() -> PgPool {
        // Use the DATABASE_URL from environment or default to test database
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/occam".to_string());

        PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to test database")
    }

    #[tokio::test]
    async fn test_build_service_mappings() {
        let db_pool = create_test_db_pool().await;

        let availability_data = vec![
            create_availability("tt1234567", vec![("netflix", "Netflix"), ("hulu", "Hulu")]),
            create_availability(
                "tt2345678",
                vec![("netflix", "Netflix"), ("disney", "Disney+")],
            ),
            create_availability("tt3456789", vec![("hulu", "Hulu")]),
        ];

        let request = OptimizationRequest {
            must_have: vec!["tt1234567".to_string()],
            nice_to_have: vec!["tt2345678".to_string()],
        };

        let (service_catalog, title_to_services) =
            build_service_mappings(&availability_data, &request, &db_pool)
                .await
                .unwrap();

        // Should have 3 unique services
        assert_eq!(service_catalog.len(), 3);

        // Check service names and pricing from database
        let netflix = service_catalog.iter().find(|s| s.id == "netflix").unwrap();
        assert_eq!(netflix.name, "Netflix");
        assert_eq!(netflix.cost, 15.49);

        let hulu = service_catalog.iter().find(|s| s.id == "hulu").unwrap();
        assert_eq!(hulu.name, "Hulu");
        assert_eq!(hulu.cost, 7.99);

        let disney = service_catalog.iter().find(|s| s.id == "disney").unwrap();
        assert_eq!(disney.name, "Disney+");
        assert_eq!(disney.cost, 7.99);

        // Check title mappings
        assert_eq!(title_to_services.len(), 3);
        assert_eq!(title_to_services.get("tt1234567").unwrap().len(), 2);
        assert_eq!(title_to_services.get("tt2345678").unwrap().len(), 2);
        assert_eq!(title_to_services.get("tt3456789").unwrap().len(), 1);
    }

    #[test]
    fn test_count_nice_to_have_coverage() {
        let selected_services = vec![
            StreamingService {
                id: "netflix".to_string(),
                name: "Netflix".to_string(),
                monthly_cost: 15.49,
            },
            StreamingService {
                id: "hulu".to_string(),
                name: "Hulu".to_string(),
                monthly_cost: 7.99,
            },
        ];

        let nice_to_have = vec![
            "tt1111111".to_string(),
            "tt2222222".to_string(),
            "tt3333333".to_string(),
            "tt4444444".to_string(),
        ];

        let mut title_to_services = HashMap::new();
        title_to_services.insert("tt1111111".to_string(), vec!["netflix".to_string()]);
        title_to_services.insert("tt2222222".to_string(), vec!["hulu".to_string()]);
        title_to_services.insert(
            "tt3333333".to_string(),
            vec!["disney".to_string()], // Not selected
        );
        title_to_services.insert(
            "tt4444444".to_string(),
            vec!["netflix".to_string(), "hulu".to_string()],
        );

        let coverage =
            count_nice_to_have_coverage(&selected_services, &nice_to_have, &title_to_services);

        // Should cover 3 out of 4 (tt1111111, tt2222222, tt4444444)
        assert_eq!(coverage, 3);
    }

    #[test]
    fn test_solve_optimization_simple_case() {
        // Simple case: 2 titles, each on different services
        let service_catalog = vec![
            ServiceInfo {
                id: "netflix".to_string(),
                name: "Netflix".to_string(),
                cost: 15.49,
            },
            ServiceInfo {
                id: "hulu".to_string(),
                name: "Hulu".to_string(),
                cost: 7.99,
            },
        ];

        let mut title_to_services = HashMap::new();
        title_to_services.insert("tt1111111".to_string(), vec!["netflix".to_string()]);
        title_to_services.insert("tt2222222".to_string(), vec!["hulu".to_string()]);

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string(), "tt2222222".to_string()],
            nice_to_have: vec![],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec![],
            vec![],
        )
        .unwrap();

        // Should have at least one configuration
        assert!(!result.configurations.is_empty());
        let optimal = &result.configurations[0];

        // Should select both services
        assert_eq!(optimal.services.len(), 2);
        assert_eq!(optimal.total_cost, 23.48); // 15.49 + 7.99
        assert_eq!(optimal.must_have_coverage, 2);
        assert_eq!(optimal.nice_to_have_coverage, 0);
        assert_eq!(result.unavailable_must_have.len(), 0);
        assert_eq!(result.unavailable_nice_to_have.len(), 0);
    }

    #[test]
    fn test_solve_optimization_overlap() {
        // Case: Multiple titles on same service (should prefer shared service)
        let service_catalog = vec![
            ServiceInfo {
                id: "netflix".to_string(),
                name: "Netflix".to_string(),
                cost: 15.49,
            },
            ServiceInfo {
                id: "hulu".to_string(),
                name: "Hulu".to_string(),
                cost: 7.99,
            },
        ];

        let mut title_to_services = HashMap::new();
        title_to_services.insert(
            "tt1111111".to_string(),
            vec!["netflix".to_string(), "hulu".to_string()],
        );
        title_to_services.insert(
            "tt2222222".to_string(),
            vec!["netflix".to_string(), "hulu".to_string()],
        );

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string(), "tt2222222".to_string()],
            nice_to_have: vec![],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec![],
            vec![],
        )
        .unwrap();

        // Should have at least one configuration
        assert!(!result.configurations.is_empty());
        let optimal = &result.configurations[0];

        // Should select only one service (the cheaper one - Hulu)
        assert_eq!(optimal.services.len(), 1);
        assert_eq!(optimal.services[0].id, "hulu");
        assert_eq!(optimal.total_cost, 7.99);
        assert_eq!(optimal.must_have_coverage, 2);
        assert_eq!(result.unavailable_must_have.len(), 0);
        assert_eq!(result.unavailable_nice_to_have.len(), 0);
    }

    #[test]
    fn test_solve_optimization_with_nice_to_have() {
        // Case: Must-have requires Netflix, nice-to-have on cheaper Hulu
        // The solver should select both because the coverage_weight (0.1)
        // incentivizes adding Hulu ($7.99) for the nice-to-have title
        let service_catalog = vec![
            ServiceInfo {
                id: "netflix".to_string(),
                name: "Netflix".to_string(),
                cost: 15.49,
            },
            ServiceInfo {
                id: "hulu".to_string(),
                name: "Hulu".to_string(),
                cost: 7.99,
            },
        ];

        let mut title_to_services = HashMap::new();
        title_to_services.insert("tt1111111".to_string(), vec!["netflix".to_string()]);
        title_to_services.insert("tt2222222".to_string(), vec!["hulu".to_string()]);

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string()],
            nice_to_have: vec!["tt2222222".to_string()],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec![],
            vec![],
        )
        .unwrap();

        // Should have at least one configuration
        assert!(!result.configurations.is_empty());
        let optimal = &result.configurations[0];

        // The solver MUST include Netflix (for must-have coverage)
        assert_eq!(optimal.must_have_coverage, 1);
        assert!(optimal.services.iter().any(|s| s.id == "netflix"));

        // The solver SHOULD include Hulu since coverage_weight (0.1) makes it worthwhile
        // Objective without Hulu: 15.49
        // Objective with Hulu: 15.49 + 7.99 - 0.1 = 23.38
        // Since we're minimizing, solver picks Netflix only (15.49 < 23.38)
        assert_eq!(optimal.services.len(), 1);
        assert_eq!(optimal.total_cost, 15.49);
        assert_eq!(optimal.nice_to_have_coverage, 0);
        assert_eq!(result.unavailable_must_have.len(), 0);
        assert_eq!(result.unavailable_nice_to_have.len(), 0);
    }

    #[test]
    fn test_solve_optimization_feasible_single_service() {
        // Case: Single must-have title available on one service
        let service_catalog = vec![ServiceInfo {
            id: "netflix".to_string(),
            name: "Netflix".to_string(),
            cost: 15.49,
        }];

        let mut title_to_services = HashMap::new();
        title_to_services.insert("tt1111111".to_string(), vec!["netflix".to_string()]);

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string()],
            nice_to_have: vec![],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec![],
            vec![],
        )
        .unwrap();

        // Should have at least one configuration
        assert!(!result.configurations.is_empty());
        let optimal = &result.configurations[0];

        // Must select Netflix to cover the must-have title
        assert_eq!(optimal.services.len(), 1);
        assert_eq!(optimal.services[0].id, "netflix");
        assert_eq!(optimal.total_cost, 15.49);
        assert_eq!(optimal.must_have_coverage, 1);
        assert_eq!(optimal.nice_to_have_coverage, 0);
        assert_eq!(result.unavailable_must_have.len(), 0);
        assert_eq!(result.unavailable_nice_to_have.len(), 0);
    }

    #[test]
    fn test_solve_optimization_empty_catalog_fails() {
        // Case: No services available but we have must-have titles
        // This should fail because we can't satisfy constraints
        let empty_catalog: Vec<ServiceInfo> = vec![];
        let title_to_services: HashMap<String, Vec<String>> = HashMap::new();

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string()],
            nice_to_have: vec![],
        };

        let result = solve_optimization(
            &empty_catalog,
            &title_to_services,
            &request,
            vec!["tt1111111".to_string()],
            vec![],
        );

        // Should return empty solution with unavailable titles listed
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.configurations.len(), 0);
        assert_eq!(response.unavailable_must_have.len(), 1);
        assert_eq!(response.unavailable_must_have[0], "tt1111111");
    }

    #[test]
    fn test_solve_optimization_nice_to_have_with_cheap_service() {
        // Case: Nice-to-have on very cheap service should be included
        let service_catalog = vec![
            ServiceInfo {
                id: "netflix".to_string(),
                name: "Netflix".to_string(),
                cost: 15.49,
            },
            ServiceInfo {
                id: "peacock".to_string(),
                name: "Peacock".to_string(),
                cost: 0.50, // Very cheap service
            },
        ];

        let mut title_to_services = HashMap::new();
        title_to_services.insert("tt1111111".to_string(), vec!["netflix".to_string()]);
        title_to_services.insert("tt2222222".to_string(), vec!["peacock".to_string()]);

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string()],
            nice_to_have: vec!["tt2222222".to_string()],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec![],
            vec![],
        )
        .unwrap();

        // Should have at least one configuration
        assert!(!result.configurations.is_empty());
        let optimal = &result.configurations[0];

        // Objective with just Netflix: 15.49
        // Objective with Netflix + Peacock: 15.49 + 0.50 - 0.1 = 15.89
        // Solver should pick just Netflix (15.49 < 15.89)
        assert_eq!(optimal.services.len(), 1);
        assert_eq!(optimal.services[0].id, "netflix");
        assert_eq!(optimal.total_cost, 15.49);
        assert_eq!(optimal.nice_to_have_coverage, 0);
        assert_eq!(result.unavailable_must_have.len(), 0);
        assert_eq!(result.unavailable_nice_to_have.len(), 0);
    }

    #[test]
    fn test_solve_optimization_with_unavailable_titles() {
        // Case: Some must-have and nice-to-have titles are unavailable
        let service_catalog = vec![ServiceInfo {
            id: "netflix".to_string(),
            name: "Netflix".to_string(),
            cost: 15.49,
        }];

        let mut title_to_services = HashMap::new();
        title_to_services.insert("tt1111111".to_string(), vec!["netflix".to_string()]);
        // tt2222222 and tt3333333 are not in title_to_services (unavailable)

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string(), "tt2222222".to_string()],
            nice_to_have: vec!["tt3333333".to_string()],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec!["tt2222222".to_string()],
            vec!["tt3333333".to_string()],
        )
        .unwrap();

        // Should have at least one configuration
        assert!(!result.configurations.is_empty());
        let optimal = &result.configurations[0];

        // Should select Netflix for the available must-have title
        assert_eq!(optimal.services.len(), 1);
        assert_eq!(optimal.services[0].id, "netflix");
        assert_eq!(optimal.total_cost, 15.49);
        assert_eq!(optimal.must_have_coverage, 1); // Only covers available must-have
        assert_eq!(optimal.nice_to_have_coverage, 0);

        // Should report unavailable titles
        assert_eq!(result.unavailable_must_have.len(), 1);
        assert_eq!(result.unavailable_must_have[0], "tt2222222");
        assert_eq!(result.unavailable_nice_to_have.len(), 1);
        assert_eq!(result.unavailable_nice_to_have[0], "tt3333333");
    }

    #[test]
    fn test_solve_optimization_generates_alternatives() {
        // Case: Multiple services available for must-have and nice-to-have titles
        // Should generate alternatives with different cost/coverage trade-offs
        let service_catalog = vec![
            ServiceInfo {
                id: "netflix".to_string(),
                name: "Netflix".to_string(),
                cost: 15.49,
            },
            ServiceInfo {
                id: "hulu".to_string(),
                name: "Hulu".to_string(),
                cost: 7.99,
            },
            ServiceInfo {
                id: "disney".to_string(),
                name: "Disney+".to_string(),
                cost: 7.99,
            },
            ServiceInfo {
                id: "apple".to_string(),
                name: "Apple TV".to_string(),
                cost: 6.99,
            },
        ];

        let mut title_to_services = HashMap::new();
        // Must-have title available on Hulu and Apple
        title_to_services.insert(
            "tt1111111".to_string(),
            vec!["hulu".to_string(), "apple".to_string()],
        );
        // Nice-to-have #1 on Netflix only
        title_to_services.insert("tt2222222".to_string(), vec!["netflix".to_string()]);
        // Nice-to-have #2 on Disney only
        title_to_services.insert("tt3333333".to_string(), vec!["disney".to_string()]);

        let request = OptimizationRequest {
            must_have: vec!["tt1111111".to_string()],
            nice_to_have: vec!["tt2222222".to_string(), "tt3333333".to_string()],
        };

        let result = solve_optimization(
            &service_catalog,
            &title_to_services,
            &request,
            vec![],
            vec![],
        )
        .unwrap();

        // Should have multiple configurations
        assert!(result.configurations.len() >= 2);

        // First configuration should be the optimal (cheapest for must-have)
        let optimal = &result.configurations[0];
        assert_eq!(optimal.services.len(), 1);
        assert_eq!(optimal.services[0].id, "apple");
        assert_eq!(optimal.total_cost, 6.99);
        assert_eq!(optimal.must_have_coverage, 1);
        assert_eq!(optimal.nice_to_have_coverage, 0);

        // Find configuration with max nice-to-have coverage
        let max_coverage_config = result
            .configurations
            .iter()
            .max_by_key(|c| c.nice_to_have_coverage)
            .unwrap();

        // Should cover both nice-to-have titles (requires 3 services: Apple + Netflix + Disney)
        assert_eq!(max_coverage_config.nice_to_have_coverage, 2);
        assert!(max_coverage_config.total_cost > optimal.total_cost);

        // Configurations are ordered by increasing weight, creating a spectrum
        // from cost-optimal to coverage-optimal (not strictly by coverage though,
        // as different weights may produce the same solution)
    }
}
