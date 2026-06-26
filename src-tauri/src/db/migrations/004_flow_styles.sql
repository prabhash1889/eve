-- Phase 6: Flow Styles. Each row tunes the polish prompt for a focused-app
-- category (resolved at record start by context::active_window). One style per
-- category (app_category UNIQUE); the pipeline looks up the active style for the
-- current category before polishing. `tone` picks a built-in voice;
-- `system_prompt` (optional) appends custom instructions; `writing_sample`
-- (optional) gives the model an example of the user's voice to imitate.

CREATE TABLE flow_styles (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT    NOT NULL DEFAULT '',                 -- display label
    app_category   TEXT    NOT NULL UNIQUE,                     -- email|workmsg|personalmsg|code|other
    tone           TEXT    NOT NULL DEFAULT 'casual',           -- casual|formal|excited|very_casual
    system_prompt  TEXT    NOT NULL DEFAULT '',                 -- extra instructions appended to the base prompt
    writing_sample TEXT    NOT NULL DEFAULT '',                 -- example of the user's voice to imitate
    is_active      INTEGER NOT NULL DEFAULT 1,                  -- 0 = disabled, skipped in pipeline
    created_at     INTEGER NOT NULL,                            -- unix epoch ms (UTC)
    updated_at     INTEGER NOT NULL
);
