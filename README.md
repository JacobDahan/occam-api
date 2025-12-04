# Occam API

Backend API for the Occam streaming service optimizer. Occam helps customers find the most cost-effective subset of streaming services to watch all the content they want.

## Features

- **Title Search**: Search for movies and TV shows with autocomplete functionality
- **Service Optimization**: Find the cheapest combination of streaming services that covers your desired content
- **Recommendations**: Get personalized watch recommendations based on your preferences

## Tech Stack

- **Framework**: Axum (Rust web framework)
- **Database**: PostgreSQL
- **Cache**: Redis
- **Solver**: Integer programming (good_lp with microlp - pure Rust MILP solver)
- **External API**: Streaming Availability API via RapidAPI

## Architecture

### System Overview

The Occam API is built with a layered architecture following clean separation of concerns:

```
┌─────────────────────────────────────────────────────────────┐
│                         Client                               │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                    Routes Layer                              │
│  (HTTP handlers: titles.rs, optimize.rs, recommendations.rs) │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                   Services Layer                             │
│  (Business logic: title_search, optimization, recommendations)│
└────────┬──────────────────────────────────┬─────────────────┘
         │                                  │
         ▼                                  ▼
┌─────────────────┐              ┌──────────────────┐
│  External APIs  │              │   Data Layer     │
│  (Streaming     │              │  (PostgreSQL +   │
│   Availability) │              │   Redis cache)   │
└─────────────────┘              └──────────────────┘
```

### Data Flow

#### 1. Title Search Flow

```
User Request → Route Handler → Service Layer → Check Redis Cache
                                                      │
                                    ┌─────────────────┴──────────────┐
                                    │                                │
                                Cache Hit                         Cache Miss
                                    │                                │
                                    ▼                                ▼
                             Return cached data           Query Streaming API
                                                                     │
                                                                     ▼
                                                          Store in Redis (TTL)
                                                                     │
                                                                     ▼
                                                              Return results
```

The title search service:
- Accepts a search query string
- Checks Redis cache for recent identical searches (key: `search:{query}`, TTL: 1 hour)
- On cache miss, queries the Streaming Availability API
- Transforms API response into our `Title` model
- Caches the results before returning

#### 2. Optimization Flow

```
User Request → Route Handler → Optimization Service
   │                               │
   │ {must_have, nice_to_have}     │
   │                               ▼
   │                    Fetch availability data for each title
   │                    (parallel API calls with Redis caching)
   │                               │
   │                               ▼
   │                    Query service pricing from PostgreSQL
   │                    (seeded data for Netflix, Hulu, etc.)
   │                               │
   │                               ▼
   │                    Build integer programming model:
   │                    - Variables: binary for each service
   │                    - Constraints: must_have titles covered
   │                    - Objective: min cost + small bonus for nice_to_have
   │                               │
   │                               ▼
   │                    Solve with microlp (pure Rust solver)
   │                               │
   │                               ▼
   │                    Extract selected services + calculate coverage
   │                               │
   └───────────────────────────────▼
                            Return optimal subset with stats
```

The optimization service:
- Receives lists of "must have" and "nice to have" IMDB IDs
- **Fetches availability data** from AvailabilityService:
  - Parallel API calls using tokio tasks for each title
  - Checks Redis cache first (key: `avail:{imdb_id}`, TTL: 1 week)
  - On cache miss, queries Streaming Availability API
  - Only considers subscription-based services (not rentals/purchases)
- **Queries service pricing** from PostgreSQL `streaming_services` table
  - Pre-seeded with current US pricing (Netflix: $15.49, Hulu: $7.99, etc.)
  - Services not in database are logged and skipped
- **Formulates integer programming problem**:
  - **Decision variables**: Binary variable for each service (0 = not selected, 1 = selected)
  - **Hard constraint**: All "must have" titles must be covered by at least one selected service
  - **Objective function**: Minimize `total_cost - 0.1 × nice_to_have_coverage`
    - Primary goal: Minimize monthly subscription cost
    - Secondary goal: Small bonus (0.1) for each nice-to-have title covered
    - Cost dominates, so solver won't add expensive services just for nice-to-haves
- **Solves using microlp** (pure Rust MILP solver, no system dependencies)
- **Returns optimal solution** with:
  - Selected streaming services with pricing
  - Total monthly cost
  - Must-have coverage count (always equals total must-haves)
  - Nice-to-have coverage count

**Example Optimization**:
```
Input:
  Must Have: [tt1375666 (Inception), tt0468569 (The Dark Knight)]
  Nice to Have: [tt0816692 (Interstellar)]

Availability Data (from API):
  - Inception: Available on Netflix, HBO Max
  - The Dark Knight: Available on HBO Max
  - Interstellar: Available on Netflix, Paramount+

Service Pricing (from database):
  - Netflix: $15.49/mo
  - HBO Max: $15.99/mo
  - Paramount+: $5.99/mo

Integer Programming Model:
  Variables: x_netflix, x_hbo, x_paramount (binary)
  Constraints:
    - Inception covered: x_netflix + x_hbo >= 1
    - Dark Knight covered: x_hbo >= 1
  Objective: Minimize (15.49·x_netflix + 15.99·x_hbo + 5.99·x_paramount - weight·coverage)

Configuration 1 (Optimal - Cost-focused):
  - Services: HBO Max only
  - Cost: $15.99/mo
  - Must-have coverage: 2/2 (100%)
  - Nice-to-have coverage: 0/1 (0%)
  - Reasoning: HBO Max covers both must-haves. Adding Netflix ($15.49)
    would cost $31.48 total for only 0.1 benefit (nice-to-have bonus).

Configuration 2 (Coverage-focused):
  - Services: HBO Max + Netflix
  - Cost: $31.48/mo
  - Must-have coverage: 2/2 (100%)
  - Nice-to-have coverage: 1/1 (100%)
  - Reasoning: Maximizes nice-to-have coverage at higher cost
```

**Configuration Generation**:
The optimizer generates service configurations by solving the optimization problem with progressively higher weights for nice-to-have coverage (0.1, 1.0, 3.0, 10.0, 100.0). This creates a natural spectrum of solutions:
- Lower weights (0.1) → Most cost-optimal solution
- Higher weights (100.0) → Most coverage-optimal solution

Duplicate configurations (same service combinations) are filtered out, resulting in up to 5 unique configurations ordered from cost-focused to coverage-focused. Users can review the spectrum and choose the configuration that best fits their budget and preferences.

#### 3. Recommendations Flow

```
User Request → Route Handler → Service Layer
   │                               │
   │ {user_titles, subscribed}     │
   │                               ▼
   │                    Fetch metadata for user titles
   │                    (genres, cast, director, ratings)
   │                               │
   │                               ▼
   │                    Find similar titles using:
   │                    - Genre overlap scoring
   │                    - Cast/director matching
   │                    - Release year proximity
   │                    - User rating correlation
   │                               │
   │                               ▼
   │                    Filter to subscribed services only
   │                               │
   │                               ▼
   │                    Rank by relevance score
   │                               │
   └───────────────────────────────▼
                            Return top 20 recommendations
```

The recommendations service:
- Accepts user's favorite titles and their subscribed services
- Fetches rich metadata for the user's titles (genres, cast, etc.)
- Uses content-based filtering to find similar titles:
  - Genre matching (cosine similarity of genre vectors)
  - Shared cast members and directors (weighted by prominence)
  - Similar release years (with decay function)
  - User rating correlation
- Filters results to only titles available on user's subscribed services
- Ranks by composite relevance score
- Returns top N recommendations

### Caching Strategy

**Redis-only** for streaming availability data:
- **Title search results**: 1 hour TTL (key: `search:{query}`)
  - Cache hit: ~4ms response time
  - Cache miss: ~2600ms (external API call)
- **Streaming availability**: 1 week TTL (key: `avail:{imdb_id}`)
  - Fetched on-demand during optimization requests
  - Parallel fetching using tokio tasks
  - Partial failures allowed (returns successful fetches)
- **API usage tracking**: Redis counters
  - Monthly quota: `api_usage:{YYYY-MM}` (25K requests/month on PRO tier)
  - Daily usage: `api_usage:daily:{YYYY-MM-DD}` (for monitoring)
  - Warning logged at 80% monthly quota

**PostgreSQL** for persistent configuration:
- **Service catalog**: `streaming_services` table
  - Pre-seeded with 10 major US services and pricing
  - Used by optimization solver (Netflix: $15.49, Hulu: $7.99, etc.)
  - Columns: id, name, base_monthly_cost, country, active
- **API usage analytics**: `api_usage_log` table (optional tracking)
- **Optimization requests**: `optimization_requests` table (future analytics)

**Why Redis-only for availability?**
- Simplicity: Single cache layer, easy to reason about
- Performance: Faster than two-tier cache (no PostgreSQL query overhead)
- Acceptable worst-case: Redis restart means re-warming cache over days (well within API quota)
- High cache hit rate: 1-week TTL gives ~80% hit rate after initial week

### Error Handling

The application uses a centralized error handling approach:
- Custom `AppError` enum maps to HTTP status codes
- Database errors → 500 Internal Server Error
- External API failures → 502 Bad Gateway
- Invalid user input → 400 Bad Request
- Missing resources → 404 Not Found
- Optimization failures → 422 Unprocessable Entity

All errors return JSON with an `error` field for client consumption.

### Request ID Tracing

The API implements comprehensive request ID tracing for tracking concurrent requests:

**Features**:
- Every request automatically gets a unique UUID v4 request ID
- Request IDs are included in all log entries via tracing spans
- Custom request IDs can be provided via the `x-request-id` header
- Request IDs are returned in the `x-request-id` response header
- Enables easy correlation of logs across distributed systems

**Usage**:
```bash
# Auto-generated request ID
curl -i http://localhost:3000/api/v1/titles/search?q=inception
# Response includes: x-request-id: 6c628d97-e9a3-40c2-9913-b8d76dc3d920

# Custom request ID (useful for distributed tracing)
curl -i -H "x-request-id: 12345678-1234-1234-1234-123456789012" \
  http://localhost:3000/api/v1/titles/search?q=matrix
# Response includes: x-request-id: 12345678-1234-1234-1234-123456789012
```

**Log Output**:
All tracing logs include the request ID, making it easy to filter concurrent requests:
```
INFO http_request{method=GET uri=/api/v1/titles/search?q=inception request_id=6c628d97-e9a3-40c2-9913-b8d76dc3d920}
INFO request_id=6c628d97-e9a3-40c2-9913-b8d76dc3d920 query=inception: Processing title search request
DEBUG request_id=6c628d97-e9a3-40c2-9913-b8d76dc3d920 query=inception: Fetching titles from external API
INFO request_id=6c628d97-e9a3-40c2-9913-b8d76dc3d920 results_count=20: Title search completed
```

### Configuration

Environment-based configuration via `.env` file:
- `DATABASE_URL`: PostgreSQL connection string
- `REDIS_URL`: Redis connection string
- `STREAMING_API_KEY`: RapidAPI key for Streaming Availability API
- `STREAMING_API_URL`: Base URL for the streaming API
- `HOST` and `PORT`: Server binding configuration

Configuration is loaded at startup using the `envy` crate for type-safe environment variable parsing.

## Getting Started

### Prerequisites

- Rust 1.88+ (run `rustup update` to upgrade)
- Docker and Docker Compose (for local PostgreSQL and Redis)
- Streaming Availability API key from [RapidAPI](https://rapidapi.com/movie-of-the-night-movie-of-the-night-default/api/streaming-availability)

### Setup

1. **Clone the repository**
   ```bash
   git clone <repository-url>
   cd occam-api
   ```

2. **Set up environment variables**
   ```bash
   cp .env.example .env
   # Edit .env and add your STREAMING_API_KEY
   ```

3. **Start PostgreSQL and Redis**
   ```bash
   docker-compose up -d
   ```

   **IMPORTANT**: SQLx requires a running PostgreSQL database at compile time to verify SQL queries. You MUST keep Postgres running during development.

4. **Build and run the API**
   ```bash
   cargo run
   ```

   The server will start on `http://127.0.0.1:3000`

### Development Commands

**IMPORTANT**: SQLx requires PostgreSQL running at compile time to verify SQL queries.

```bash
# First-time setup
docker-compose up -d postgres redis
cargo install sqlx-cli --no-default-features --features postgres
cargo sqlx migrate run

# Build and run the API locally
cargo run

# Run tests
cargo test

# Check code compiles (faster than full build)
cargo check
```

**Common Operations:**

```bash
# Check service status
docker-compose ps

# View logs
docker-compose logs postgres
docker-compose logs redis

# Stop services (keeps data)
docker-compose down

# Wipe all data and start fresh
docker-compose down -v
docker-compose up -d postgres redis

# Clear Redis cache only
docker-compose exec redis redis-cli FLUSHALL

# Rebuild and run API in Docker
docker-compose up -d --build
docker-compose logs -f api

# Health check
curl http://localhost:3000/health
```

## API Endpoints

### Health Check
```
GET /health
```

### Title Search
```bash
GET /api/v1/titles/search?q=inception
```

**Status**: ✅ **Implemented**

Example response:
```json
[
  {
    "id": "70",
    "imdb_id": "tt1375666",
    "title": "Inception",
    "title_type": "movie",
    "release_year": 2010,
    "overview": "A thief steals people's secrets from their subconscious while they dream."
  }
]
```

Features:
- Searches Streaming Availability API for titles matching query
- Returns up to 20 results
- Caches results in Redis for 1 hour
- Cache hits return in ~4ms vs ~2600ms for API calls
- Validates non-empty queries
- Supports both movies and series

### Optimization
```bash
POST /api/v1/optimize
Content-Type: application/json

{
  "must_have": ["tt1375666"],  // Inception IMDB ID
  "nice_to_have": ["tt0468569"]  // The Dark Knight IMDB ID
}
```

**Status**: ✅ **Implemented**

Example response:
```json
{
  "configurations": [
    {
      "services": [
        {
          "id": "hbo_max",
          "name": "Max",
          "monthly_cost": 15.99
        }
      ],
      "total_cost": 15.99,
      "must_have_coverage": 2,
      "nice_to_have_coverage": 0
    },
    {
      "services": [
        {
          "id": "hbo_max",
          "name": "Max",
          "monthly_cost": 15.99
        },
        {
          "id": "netflix",
          "name": "Netflix",
          "monthly_cost": 15.49
        }
      ],
      "total_cost": 31.48,
      "must_have_coverage": 2,
      "nice_to_have_coverage": 1
    }
  ],
  "unavailable_must_have": [],
  "unavailable_nice_to_have": []
}
```

Features:
- Fetches streaming availability from Streaming Availability API
- Parallel API calls with 1-week Redis caching
- Queries service pricing from PostgreSQL
- Solves integer programming problem using microlp (pure Rust)
- Prioritizes cost minimization over nice-to-have coverage
- Returns optimal service subset with coverage statistics
- **Returns ordered list of service configurations (cost-optimal to coverage-optimal)**
- **Up to 5 unique configurations with different cost/coverage trade-offs**
- Graceful handling of partial API failures
- Rate limiting with quota tracking (25K requests/month)

### Recommendations
```
POST /api/v1/recommendations
Content-Type: application/json

{
  "user_titles": ["tt1375666", "tt0468569"],
  "subscribed_services": ["netflix", "hulu"]
}
```

## Project Structure

```
occam-api/
├── src/
│   ├── main.rs              # Application entry point
│   ├── config.rs            # Configuration management
│   ├── error.rs             # Error handling
│   ├── models/
│   │   └── mod.rs           # Data models (Title, StreamingAvailability, etc.)
│   ├── db/                  # Database and cache connections
│   ├── middleware/          # HTTP middleware
│   │   └── request_id.rs    # Request ID generation and tracing
│   ├── routes/              # HTTP route handlers
│   │   ├── mod.rs           # AppState and router setup
│   │   ├── titles.rs        # Title search endpoint
│   │   ├── optimize.rs      # Optimization endpoint
│   │   └── recommendations.rs
│   └── services/            # Business logic
│       ├── title_search.rs  # Title search with Redis caching
│       ├── availability.rs  # Streaming availability fetching
│       ├── optimization.rs  # Integer programming solver
│       └── recommendations.rs
├── migrations/              # Database migrations
│   ├── 001_create_availability_schema.sql
│   └── 002_seed_streaming_services.sql
├── Dockerfile               # Multi-stage Rust build
└── docker-compose.yml       # PostgreSQL, Redis, and API services
```

## Implementation Status

### Completed ✅

1. **Title Search Service**: Fully implemented with Redis caching
   - Streaming Availability API integration
   - 1-hour Redis cache with automatic TTL
   - Input validation and error handling
   - Unit tests for model conversion
   - Trait-based design for easy mocking

2. **Availability Service**: Streaming availability data fetching
   - Parallel API calls using tokio tasks
   - Redis-only caching (1-week TTL)
   - Rate limiting with quota tracking (25K/month)
   - Graceful partial failure handling
   - Only includes pricing for rentals/purchases (NOT subscriptions)
   - Unit tests for API response conversion and type filtering

3. **Optimization Service**: Integer programming solver
   - Database-sourced service pricing (PostgreSQL)
   - On-demand availability fetching with caching
   - Integer programming using microlp (pure Rust, no system dependencies)
   - Cost-optimized service selection with nice-to-have bonus
   - **Generates up to 5 configurations with increasing weights (0.1, 1.0, 3.0, 10.0, 100.0)**
   - **Configuration deduplication to remove duplicate service combinations**
   - Comprehensive unit tests (9 tests, all deterministic)
   - Performance: 105-800ms total optimization time

4. **Database Schema**: PostgreSQL tables
   - `streaming_services`: Service catalog with pricing (10 pre-seeded services)
   - `api_usage_log`: API call tracking for analytics
   - `optimization_requests`: Request history for future analytics

5. **Docker Deployment**: Multi-stage containerization
   - Rust builder stage with optimized release build
   - Debian slim runtime with minimal dependencies
   - Docker Compose orchestration (PostgreSQL, Redis, API)
   - Health checks for all services
   - Proper networking (0.0.0.0 binding for container access)

### In Progress 🚧

**Recommendations Service**: Not yet implemented

### To Do 📋

#### Core Features
1. **Recommendations Service**: Build content-based filtering algorithm

#### Production Hardening
2. **Optimization Service**:
   - Add input validation (max titles per request)
   - Handle edge cases (no services available for must-haves)
   - Return 429 when quota exceeded
3. **Availability Service**:
   - Retry logic for transient API failures
   - Circuit breaker pattern for API outages
   - Better cache failure handling (continue with API if Redis fails)
4. **Metrics & Monitoring**:
   - Cache hit/miss rates
   - API quota consumption trends
   - Optimization solve times
   - Track latency percentiles
5. **Configuration**:
   - Make cache TTL configurable via environment variables
   - Configurable nice-to-have weight (currently hardcoded 0.1)
   - Rate limit thresholds

#### Future Enhancements
6. **Multi-region Support**: Add country parameter to optimization requests
7. **24-hour Freshness**: Reduce availability cache TTL for fresher data
8. **Background Refresh**: Pre-warm cache for popular titles
9. **Analytics Dashboard**: Query optimization_requests table for usage patterns

## Development

### Code Quality
- Run `cargo check` to verify code compiles
- Run `cargo fmt` to format code
- Run `cargo clippy` to lint code

### Testing

The project uses a comprehensive testing approach:

**Unit Tests** (require Docker containers for database tests):
```bash
# Start required services
docker-compose up -d postgres redis

# Run all tests
cargo test --release
```

**Test Coverage**:
- **Title Search Service** (4 tests):
  - API response conversion (ApiShow → Title)
  - Missing data handling
  - Empty/whitespace query validation
  - Mock-based service testing with trait abstraction

- **Availability Service** (4 tests):
  - API response parsing and type conversion
  - Filtering unknown availability types
  - Price exclusion for subscriptions (only rentals/purchases have prices)
  - All types handled correctly (Subscription, Rent, Buy, Free, Addon)

- **Optimization Service** (9 tests):
  - Service mapping with database pricing lookup
  - Nice-to-have coverage calculation
  - Simple optimization (disjoint services)
  - Overlapping service selection (prefers cheaper option)
  - Nice-to-have behavior (cost dominates, won't add expensive services)
  - Single service feasibility
  - Empty catalog error handling
  - Cheap service cost-benefit analysis
  - Multiple configurations generation with different weights

**Testing Principles**:
- All tests are deterministic with exact assertions (no vague "could be 1 or 2" comments)
- Database-backed tests use real PostgreSQL with seeded data
- No mocking of database or Redis in optimization tests (integration style)
- Availability service tests are pure functions (no external dependencies)
- Tests document expected behavior with mathematical reasoning
- Fast execution: All 17+ tests complete in ~20-50ms

## License

See LICENSE file for details.
