-- What turns a project into work the engine can suggest.
ALTER TABLE projects ADD COLUMN definition_of_done TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN next_action TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN next_action_minutes INTEGER NOT NULL DEFAULT 0;
-- Bumped whenever the next action changes, so each next action is its own
-- task: finishing one never hides the one after it.
ALTER TABLE projects ADD COLUMN next_action_rev INTEGER NOT NULL DEFAULT 0;
