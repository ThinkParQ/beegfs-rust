-- This can be NULL as it only applies to storage groups
ALTER table buddy_groups ADD COLUMN quota_accounting INTEGER;
-- Make sure existing mirrored targets don't change behavior automatically
UPDATE buddy_groups SET quota_accounting = 2 WHERE node_type = 2;

ALTER table buddy_groups ADD CONSTRAINT quota_accounting_null
CHECK ((node_type == 2) == (quota_accounting IS NOT NULL));
