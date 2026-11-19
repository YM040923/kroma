-- Technical probe details beyond codec and size: container-global stream
-- indexes, frame rate, per-track bit rates, sample rates and colour transfer.
-- A successful probe's schema version tells the module when a sidecar is
-- complete enough not to be fetched again.
ALTER TABLE files ADD COLUMN v_technical TEXT;
ALTER TABLE files ADD COLUMN probe_version INTEGER;
-- Season-level text and artwork, separate from the episodes on disk.
ALTER TABLE season_meta ADD COLUMN metadata TEXT;
-- Production companies and broadcast networks supplied by the provider.
ALTER TABLE metadata_core ADD COLUMN studios TEXT NOT NULL DEFAULT '[]';
