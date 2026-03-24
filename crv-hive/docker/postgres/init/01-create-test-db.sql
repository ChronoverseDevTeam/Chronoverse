SELECT 'CREATE DATABASE chronoverse_test'
WHERE NOT EXISTS (
    SELECT FROM pg_database WHERE datname = 'chronoverse_test'
)\gexec