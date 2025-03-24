-- Event counts by status
SELECT status->>'type' as status, count(*) 
FROM events 
GROUP BY status->>'type';

-- Recent events
SELECT tx_hash, 
       status->>'type' as status,
       created_at,
       EXTRACT(EPOCH FROM (now() - created_at)) as age_seconds
FROM events 
ORDER BY created_at DESC 
LIMIT 10; 