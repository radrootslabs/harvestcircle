CREATE TABLE application_schema (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version >= 1)
);

INSERT INTO application_schema (singleton, schema_version) VALUES (1, 1);
