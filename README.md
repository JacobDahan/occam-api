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
- **Solver**: Integer programming (good_lp with CBC solver)
- **External API**: Streaming Availability API

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
User Request → Route Handler → Service Layer
   │                               │
   │ {must_have, nice_to_have}     │
   │                               ▼
   │                    Query availability data for each title
   │                    (cached in PostgreSQL from previous searches)
   │                               │
   │                               ▼
   │                    Build integer programming model:
   │                    - Variables: binary for each service
   │                    - Constraints: must_have titles covered
   │                    - Objective: min cost, max nice_to_have
   │                               │
   │                               ▼
   │                    Solve with CBC solver (good_lp)
   │                               │
   │                               ▼
   │                    Generate alternatives (relaxed constraints)
   │                               │
   └───────────────────────────────▼
                            Return optimal subset + alternatives
```

The optimization service:
- Receives lists of "must have" and "nice to have" title IDs
- Queries which streaming services have each title (from cache/database)
- Formulates as an integer programming problem:
  - **Decision variables**: Binary variable for each service (selected or not)
  - **Hard constraint**: All "must have" titles must be covered
  - **Primary objective**: Minimize total monthly cost
  - **Secondary objective**: Maximize "nice to have" titles covered
- Uses the CBC (COIN-OR Branch and Cut) solver via good_lp
- Generates 2-3 alternative solutions with different cost/coverage trade-offs
- Returns the optimal solution with alternatives

**Example Optimization**:
```
Must Have: [Title A, Title B]
Nice to Have: [Title C, Title D]

Services:
- Netflix ($15/mo): Has A, C
- Hulu ($8/mo): Has B, D
- HBO Max ($16/mo): Has A, B, C

Optimal Solution: Netflix + Hulu
- Cost: $23/mo
- Coverage: All must-haves + all nice-to-haves
- Alternative: HBO Max alone ($16) covers must-haves but only 1 nice-to-have
```

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

**Redis** is used for short-term, high-frequency data:
- API search results (1 hour TTL)
- Streaming availability lookups (24 hour TTL)
- Rate limiting counters

**PostgreSQL** is used for persistent data:
- Title metadata (synced from API)
- Service pricing and availability mappings
- User preferences and history (future feature)
- Optimization results for analytics

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

4. **Build and run the API**
   ```bash
   cargo run
   ```

   The server will start on `http://127.0.0.1:3000`

### Testing

Run the health check endpoint:
```bash
curl http://localhost:3000/health
```

Expected response:
```json
{"status": "healthy"}
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
```
POST /api/v1/optimize
Content-Type: application/json

{
  "must_have": ["tt1375666"],  // Inception IMDB ID
  "nice_to_have": ["tt0468569"]  // The Dark Knight IMDB ID
}
```

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
│   ├── models/              # Data models
│   ├── db/                  # Database and cache connections
│   ├── middleware/          # HTTP middleware
│   │   └── request_id.rs    # Request ID generation and tracing
│   ├── routes/              # HTTP route handlers
│   └── services/            # Business logic
│       ├── title_search.rs
│       ├── optimization.rs
│       └── recommendations.rs
├── migrations/              # Database migrations
└── docker-compose.yml       # Local development services
```

## Implementation Status

### Completed ✅
1. **Title Search Service**: Fully implemented with Redis caching
   - Streaming Availability API integration
   - 1-hour Redis cache with automatic TTL
   - Input validation and error handling
   - Unit tests for model conversion
   - Trait-based design for easy mocking

### In Progress 🚧
2. **Optimization Service**: Not yet implemented
3. **Recommendations Service**: Not yet implemented

### To Do 📋

#### Core Features
1. **Optimization Service**: Implement integer programming solver
2. **Recommendations Service**: Build recommendation algorithm
3. **Database Schema**: Create tables for persistent title/service data
4. **Service Data**: Map streaming service pricing and availability

#### Production Hardening (Title Search)
5. **Rate Limiting**: Track API quota usage in Redis, return 429 when approaching limits
6. **Metrics & Monitoring**:
   - Count API calls, cache hits/misses, errors
   - Log quota remaining
   - Track latency percentiles
7. **Configuration**: Make cache TTL configurable via environment variables
8. **Error Recovery**:
   - Add retry logic for transient API failures
   - Implement circuit breaker pattern
   - Better cache failure handling
9. **API Enhancements**:
   - Support pagination for large result sets
   - Add result filtering/sorting options
   - Allow configurable result limits

## Development

### Code Quality
- Run `cargo check` to verify code compiles
- Run `cargo fmt` to format code
- Run `cargo clippy` to lint code

### Testing

The project uses a two-tier testing approach:

**Unit Tests** (no external dependencies):
```bash
cargo test
```
- Uses mockall for mocking external dependencies
- Tests run in milliseconds
- No Redis or external API required
- Includes model conversion and validation logic

**Integration Tests** (requires Redis):
```bash
cargo test -- --ignored
```
- Tests with real Redis connections
- Marked with `#[ignore]` attribute
- Run separately to avoid CI/local environment issues

**Test Coverage**:
- Model conversion (ApiShow → Title)
- Input validation (empty/whitespace queries)
- Mock-based service behavior testing
- Integration tests for real service instances

## License

See LICENSE file for details.
