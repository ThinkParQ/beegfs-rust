-- We don't care about existing nic entries as they are replaced at start anyway.
DELETE FROM node_nics;

-- Replace binary ip address with text form
ALTER TABLE node_nics DROP COLUMN addr;
ALTER TABLE node_nics ADD COLUMN addr TEXT NOT NULL;
