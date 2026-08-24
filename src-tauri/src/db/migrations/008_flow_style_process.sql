-- 4.3: exact-app Flow Style profiles. Rebuilds flow_styles so uniqueness is
-- (app_category, app_process): app_process '' keeps the whole-category default
-- row, any other value scopes a style to exactly that process name (stored
-- lowercased), which the pipeline checks BEFORE the category fallback.

CREATE TABLE flow_styles_new (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT    NOT NULL DEFAULT '',                 -- display label
    app_category   TEXT    NOT NULL,                            -- email|workmsg|personalmsg|code|other
    app_process    TEXT    NOT NULL DEFAULT '',                 -- '' = whole category; else exact process (lowercased)
    tone           TEXT    NOT NULL DEFAULT 'casual',           -- casual|formal|excited|very_casual
    system_prompt  TEXT    NOT NULL DEFAULT '',                 -- extra instructions appended to the base prompt
    writing_sample TEXT    NOT NULL DEFAULT '',                 -- example of the user's voice to imitate
    is_active      INTEGER NOT NULL DEFAULT 1,                  -- 0 = disabled, skipped in pipeline
    created_at     INTEGER NOT NULL,                            -- unix epoch ms (UTC)
    updated_at     INTEGER NOT NULL,
    UNIQUE(app_category, app_process)
);

INSERT INTO flow_styles_new
    (name, app_category, app_process, tone, system_prompt, writing_sample, is_active, created_at, updated_at)
SELECT name, app_category, '', tone, system_prompt, writing_sample, is_active, created_at, updated_at
FROM flow_styles;

DROP TABLE flow_styles;
ALTER TABLE flow_styles_new RENAME TO flow_styles;
