-- Create streaming services catalog table
CREATE TABLE streaming_services (
    id VARCHAR(50) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    base_monthly_cost DECIMAL(10, 2) NOT NULL,
    country VARCHAR(2) DEFAULT 'US' NOT NULL,
    active BOOLEAN DEFAULT true NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

-- Create API usage log for quota tracking
CREATE TABLE api_usage_log (
    id SERIAL PRIMARY KEY,
    endpoint VARCHAR(100) NOT NULL,
    request_count INTEGER DEFAULT 1 NOT NULL,
    date DATE NOT NULL DEFAULT CURRENT_DATE,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    UNIQUE(endpoint, date)
);

CREATE INDEX idx_api_usage_date ON api_usage_log(date);

-- Create optimization requests table for analytics
CREATE TABLE optimization_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id UUID NOT NULL,
    must_have_titles TEXT[] NOT NULL,
    nice_to_have_titles TEXT[] NOT NULL,
    cache_hit_rate DECIMAL(5, 2),
    api_calls_made INTEGER DEFAULT 0 NOT NULL,
    total_cost DECIMAL(10, 2),
    services_selected TEXT[],
    processing_time_ms INTEGER,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

CREATE INDEX idx_optimization_requests_created ON optimization_requests(created_at);
CREATE INDEX idx_optimization_requests_request_id ON optimization_requests(request_id);
