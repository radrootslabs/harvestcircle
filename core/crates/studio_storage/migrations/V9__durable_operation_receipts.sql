ALTER TABLE durable_operations ADD COLUMN prior_binding_availability TEXT CHECK (
    prior_binding_availability IS NULL OR prior_binding_availability IN (
        'available',
        'credential_missing',
        'store_unavailable'
    )
);

ALTER TABLE durable_operations ADD COLUMN resulting_revision INTEGER CHECK (
    resulting_revision IS NULL OR resulting_revision >= 0
);
