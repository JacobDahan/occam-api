-- Add Watchmode service ID mapping column
ALTER TABLE streaming_services
ADD COLUMN watchmode_service_id INTEGER;

-- Update existing services with Watchmode IDs
UPDATE streaming_services SET watchmode_service_id = 203 WHERE id = 'netflix';
UPDATE streaming_services SET watchmode_service_id = 157 WHERE id = 'hulu';
UPDATE streaming_services SET watchmode_service_id = 26 WHERE id = 'prime';
UPDATE streaming_services SET watchmode_service_id = 372 WHERE id = 'disney';
UPDATE streaming_services SET watchmode_service_id = 387 WHERE id = 'hbo';
UPDATE streaming_services SET watchmode_service_id = 371 WHERE id = 'apple';
UPDATE streaming_services SET watchmode_service_id = 444 WHERE id = 'paramount';
UPDATE streaming_services SET watchmode_service_id = 389 WHERE id = 'peacock';
UPDATE streaming_services SET watchmode_service_id = 232 WHERE id = 'starz';

-- Create index for efficient Watchmode ID lookups
CREATE INDEX idx_streaming_services_watchmode_id ON streaming_services(watchmode_service_id);
