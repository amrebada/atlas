-- 0006_project_disk_bytes.sql
-- Store the full on-disk footprint of each project alongside the existing
-- `size_bytes` (which respects .gitignore). Pair lets the UI distinguish
-- "source size" from "disk usage" without a live scan.

ALTER TABLE projects ADD COLUMN disk_bytes INTEGER NOT NULL DEFAULT 0;
