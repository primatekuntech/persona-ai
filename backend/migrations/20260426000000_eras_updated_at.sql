-- Add updated_at to eras so PATCH can use it as the dynamic-query anchor.
ALTER TABLE eras ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
