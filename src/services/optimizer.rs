use std::collections::HashSet;

use thiserror::Error;
use uuid::Uuid;

use crate::models::{StreamingService, UserPreferences};

/// Error types for the optimizer
#[derive(Debug, Error)]
pub enum OptimizerError {
    #[error("No solution exists: unable to cover all must-have titles")]
    NoSolution,
    #[error("No streaming services available")]
    NoServices,
}

/// Result of the optimization process
#[derive(Debug, Clone, PartialEq)]
pub struct OptimizationResult {
    /// Recommended services to subscribe to
    pub recommended_services: Vec<Uuid>,
    /// Total monthly cost in cents
    pub total_monthly_cost_cents: u32,
    /// Must-have titles covered
    pub must_have_covered: Vec<Uuid>,
    /// Nice-to-have titles covered
    pub nice_to_have_covered: Vec<Uuid>,
    /// Titles that cannot be covered by any available service
    pub unavailable_titles: Vec<Uuid>,
}

/// Optimizer for finding the best streaming service subset
pub struct Optimizer<'a> {
    services: &'a [StreamingService],
    preferences: &'a UserPreferences,
}

impl<'a> Optimizer<'a> {
    /// Creates a new optimizer with given services and user preferences
    pub fn new(services: &'a [StreamingService], preferences: &'a UserPreferences) -> Self {
        Self {
            services,
            preferences,
        }
    }

    /// Finds the optimal subset of streaming services
    /// Uses a greedy algorithm that prioritizes covering must-have titles first,
    /// then maximizes nice-to-have coverage while minimizing cost
    pub fn optimize(&self) -> Result<OptimizationResult, OptimizerError> {
        if self.services.is_empty() {
            return Err(OptimizerError::NoServices);
        }

        let must_have_titles: HashSet<Uuid> = self.preferences.must_have_titles().into_iter().collect();
        let nice_to_have_titles: HashSet<Uuid> = self.preferences.nice_to_have_titles().into_iter().collect();
        let current_subs: HashSet<Uuid> = self.preferences.current_subscriptions.iter().copied().collect();

        // Start with current subscriptions
        let mut selected_services: HashSet<Uuid> = HashSet::new();
        let mut covered_titles: HashSet<Uuid> = HashSet::new();

        // Add currently subscribed services first
        for service in self.services.iter() {
            if current_subs.contains(&service.id) {
                selected_services.insert(service.id);
                for title_id in &service.available_titles {
                    covered_titles.insert(*title_id);
                }
            }
        }

        // Find which titles are not available on any service
        let all_available: HashSet<Uuid> = self.services
            .iter()
            .flat_map(|s| s.available_titles.iter().copied())
            .collect();

        let unavailable_must_have: Vec<Uuid> = must_have_titles
            .iter()
            .filter(|t| !all_available.contains(t))
            .copied()
            .collect();

        let unavailable_nice_to_have: Vec<Uuid> = nice_to_have_titles
            .iter()
            .filter(|t| !all_available.contains(t))
            .copied()
            .collect();

        // Check if all must-have titles that are available can be covered
        let available_must_have: HashSet<Uuid> = must_have_titles
            .iter()
            .filter(|t| all_available.contains(t))
            .copied()
            .collect();

        // Greedy algorithm to cover remaining must-have titles
        let mut uncovered_must_have: HashSet<Uuid> = available_must_have
            .iter()
            .filter(|t| !covered_titles.contains(t))
            .copied()
            .collect();

        while !uncovered_must_have.is_empty() {
            // Find the service with best value: covers most uncovered must-haves per dollar
            let best_service = self.services
                .iter()
                .filter(|s| !selected_services.contains(&s.id))
                .max_by(|a, b| {
                    let a_coverage = a.available_titles.iter().filter(|t| uncovered_must_have.contains(t)).count();
                    let b_coverage = b.available_titles.iter().filter(|t| uncovered_must_have.contains(t)).count();
                    
                    // Compare by coverage first, then by cost (prefer cheaper)
                    let a_score = if a.monthly_cost_cents == 0 { f64::MAX } else { a_coverage as f64 / a.monthly_cost_cents as f64 };
                    let b_score = if b.monthly_cost_cents == 0 { f64::MAX } else { b_coverage as f64 / b.monthly_cost_cents as f64 };
                    
                    a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
                });

            match best_service {
                Some(service) if service.available_titles.iter().any(|t| uncovered_must_have.contains(t)) => {
                    selected_services.insert(service.id);
                    for title_id in &service.available_titles {
                        covered_titles.insert(*title_id);
                        uncovered_must_have.remove(title_id);
                    }
                }
                _ => {
                    // No service can cover remaining must-haves
                    return Err(OptimizerError::NoSolution);
                }
            }
        }

        // Now greedily add services for nice-to-have titles if cost-effective
        let mut uncovered_nice_to_have: HashSet<Uuid> = nice_to_have_titles
            .iter()
            .filter(|t| all_available.contains(t) && !covered_titles.contains(t))
            .copied()
            .collect();

        // Continue adding services that provide good value for nice-to-haves
        while !uncovered_nice_to_have.is_empty() {
            let best_service = self.services
                .iter()
                .filter(|s| !selected_services.contains(&s.id))
                .filter(|s| s.available_titles.iter().any(|t| uncovered_nice_to_have.contains(t)))
                .max_by(|a, b| {
                    let a_coverage = a.available_titles.iter().filter(|t| uncovered_nice_to_have.contains(t)).count();
                    let b_coverage = b.available_titles.iter().filter(|t| uncovered_nice_to_have.contains(t)).count();
                    
                    let a_score = if a.monthly_cost_cents == 0 { f64::MAX } else { a_coverage as f64 / a.monthly_cost_cents as f64 };
                    let b_score = if b.monthly_cost_cents == 0 { f64::MAX } else { b_coverage as f64 / b.monthly_cost_cents as f64 };
                    
                    a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
                });

            match best_service {
                Some(service) => {
                    selected_services.insert(service.id);
                    for title_id in &service.available_titles {
                        covered_titles.insert(*title_id);
                        uncovered_nice_to_have.remove(title_id);
                    }
                }
                None => break,
            }
        }

        // Calculate results
        let total_cost: u32 = self.services
            .iter()
            .filter(|s| selected_services.contains(&s.id))
            .map(|s| s.monthly_cost_cents)
            .sum();

        let must_have_covered: Vec<Uuid> = must_have_titles
            .iter()
            .filter(|t| covered_titles.contains(t))
            .copied()
            .collect();

        let nice_to_have_covered: Vec<Uuid> = nice_to_have_titles
            .iter()
            .filter(|t| covered_titles.contains(t))
            .copied()
            .collect();

        let mut unavailable_titles = unavailable_must_have;
        unavailable_titles.extend(unavailable_nice_to_have);

        Ok(OptimizationResult {
            recommended_services: selected_services.into_iter().collect(),
            total_monthly_cost_cents: total_cost,
            must_have_covered,
            nice_to_have_covered,
            unavailable_titles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Priority, StreamingService};

    fn create_test_services() -> (Vec<StreamingService>, Vec<Uuid>) {
        // Create some test titles
        let title1 = Uuid::new_v4(); // The Matrix
        let title2 = Uuid::new_v4(); // Inception
        let title3 = Uuid::new_v4(); // Breaking Bad
        let title4 = Uuid::new_v4(); // The Office
        let title5 = Uuid::new_v4(); // Stranger Things

        let mut netflix = StreamingService::new("Netflix".to_string(), 1599);
        netflix.add_title(title1);
        netflix.add_title(title3);
        netflix.add_title(title5);

        let mut hulu = StreamingService::new("Hulu".to_string(), 999);
        hulu.add_title(title2);
        hulu.add_title(title4);

        let mut prime = StreamingService::new("Prime Video".to_string(), 1499);
        prime.add_title(title1);
        prime.add_title(title2);

        (vec![netflix, hulu, prime], vec![title1, title2, title3, title4, title5])
    }

    #[test]
    fn test_empty_services() {
        let prefs = UserPreferences::new();
        let optimizer = Optimizer::new(&[], &prefs);
        let result = optimizer.optimize();
        assert!(matches!(result, Err(OptimizerError::NoServices)));
    }

    #[test]
    fn test_no_preferences() {
        let (services, _) = create_test_services();
        let prefs = UserPreferences::new();
        let optimizer = Optimizer::new(&services, &prefs);
        let result = optimizer.optimize().unwrap();
        assert!(result.recommended_services.is_empty());
        assert_eq!(result.total_monthly_cost_cents, 0);
    }

    #[test]
    fn test_single_must_have() {
        let (services, titles) = create_test_services();
        let mut prefs = UserPreferences::new();
        prefs.add_title(titles[0], Priority::MustHave); // The Matrix (on Netflix and Prime)

        let optimizer = Optimizer::new(&services, &prefs);
        let result = optimizer.optimize().unwrap();

        // Should pick either Netflix or Prime (whichever is more cost-effective)
        assert_eq!(result.recommended_services.len(), 1);
        assert_eq!(result.must_have_covered.len(), 1);
        assert!(result.must_have_covered.contains(&titles[0]));
    }

    #[test]
    fn test_must_have_across_services() {
        let (services, titles) = create_test_services();
        let mut prefs = UserPreferences::new();
        prefs.add_title(titles[2], Priority::MustHave); // Breaking Bad (only on Netflix)
        prefs.add_title(titles[3], Priority::MustHave); // The Office (only on Hulu)

        let optimizer = Optimizer::new(&services, &prefs);
        let result = optimizer.optimize().unwrap();

        // Must have both Netflix and Hulu
        assert_eq!(result.recommended_services.len(), 2);
        assert_eq!(result.must_have_covered.len(), 2);
    }

    #[test]
    fn test_unavailable_title() {
        let (services, _) = create_test_services();
        let unavailable_title = Uuid::new_v4();
        let mut prefs = UserPreferences::new();
        prefs.add_title(unavailable_title, Priority::NiceToHave);

        let optimizer = Optimizer::new(&services, &prefs);
        let result = optimizer.optimize().unwrap();

        assert!(result.unavailable_titles.contains(&unavailable_title));
    }

    #[test]
    fn test_current_subscription_considered() {
        let (services, titles) = create_test_services();
        let mut prefs = UserPreferences::new();
        prefs.add_title(titles[0], Priority::MustHave); // The Matrix
        prefs.add_subscription(services[0].id); // Already subscribed to Netflix

        let optimizer = Optimizer::new(&services, &prefs);
        let result = optimizer.optimize().unwrap();

        // Should keep Netflix since already subscribed and it covers the title
        assert!(result.recommended_services.contains(&services[0].id));
    }
}
