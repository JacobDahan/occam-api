-- Seed common US streaming services with current pricing (as of December 2025)
-- Service IDs match Streaming Availability API identifiers
-- Prices reflect the lowest ad-free tier for each service
INSERT INTO streaming_services (id, name, base_monthly_cost, country) VALUES
    ('netflix', 'Netflix', 17.99, 'US'),        -- Standard (No Ads) plan
    ('hulu', 'Hulu', 18.99, 'US'),              -- No Ads plan
    ('prime', 'Amazon Prime Video', 11.98, 'US'), -- Standalone + ad-free tier ($8.99 + $2.99)
    ('disney', 'Disney+', 18.99, 'US'),         -- Premium (No Ads) plan
    ('hbo', 'Max', 18.49, 'US'),                -- Standard (Ad-Free) plan
    ('apple', 'Apple TV+', 12.99, 'US'),        -- Ad-free only service
    ('paramount', 'Paramount+', 12.99, 'US'),   -- Premium (includes Showtime, ad-free)
    ('peacock', 'Peacock', 16.99, 'US'),        -- Premium Plus (No Ads) plan
    ('starz', 'Starz', 10.99, 'US')             -- Standard monthly subscription
ON CONFLICT (id) DO NOTHING;
