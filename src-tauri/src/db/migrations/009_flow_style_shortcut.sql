-- 4.4: dictation-style override per hotkey. An optional global accelerator on a
-- Flow Style row: pressing it arms that style for the NEXT dictation only
-- ("dictate as email"), overriding both the exact-app profile and the category
-- default. Empty = no accelerator.

ALTER TABLE flow_styles ADD COLUMN shortcut TEXT NOT NULL DEFAULT '';
