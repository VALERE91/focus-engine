-- The task's category at the time of the outcome, for the daily growth
-- share. Earlier outcomes were all videos from leisure-only channels.
ALTER TABLE outcomes ADD COLUMN category TEXT NOT NULL DEFAULT 'leisure';
