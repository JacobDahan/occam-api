/// A macro to simplify caching logic using Redis.
///
/// This macro checks if a value is present in the cache.
/// If found, it returns the cached value.
/// If not found, it executes the provided block to compute the value,
/// stores it in the cache, and then returns the computed value.
///
/// # Arguments
/// * `$cache`: The cache instance to use for retrieval and storage. The cache must have
///   `get_from_cache` and `set_in_background` methods.
/// * `$key`: The key to use for caching the value.
/// * `$ttl`: The time-to-live (TTL) for the cached value in seconds.
/// * `$block`: The block of code to execute if the value is not found in cache.
///
/// # Example
/// ```rust,no_run
/// let cached_value = cached!(cache, cache_key, async move {
///    // Compute the value if not in cache
///   compute_expensive_value()
/// });
/// ```
#[macro_export]
macro_rules! cached {
    ($cache:expr, $key:expr, $ttl:expr, $block:expr) => {{
        // Attempt to get the value from cache
        if let Some(cached) = $cache.get_from_cache(&$key).await? {
            Ok(cached)
        } else {
            // If not in cache, execute the block to compute the value
            let value = $block.await?;
            // Store the computed value in cache
            $cache.set_in_background(&$key, &value, $ttl);
            Ok(value)
        }
    }};
}
