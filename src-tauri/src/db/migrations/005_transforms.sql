-- Phase 7: Transforms. A transform is a saved rewrite prompt the user can run
-- on selected text via a bound shortcut, or have auto-applied after dictation.
-- `system_prompt` is the instruction sent to the LLM; `shortcut` (optional) is a
-- global accelerator registered at launch; `auto_apply` runs the transform on
-- every dictation; `app_category` (optional) scopes auto-apply to a focused-app
-- category (empty = all apps).

CREATE TABLE transforms (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT    NOT NULL,                       -- display label
    system_prompt TEXT    NOT NULL DEFAULT '',            -- rewrite instruction sent to the LLM
    shortcut      TEXT    NOT NULL DEFAULT '',            -- optional global accelerator
    auto_apply    INTEGER NOT NULL DEFAULT 0,             -- 1 = run after every dictation
    app_category  TEXT    NOT NULL DEFAULT '',            -- scope auto-apply to a category (empty = all)
    is_active     INTEGER NOT NULL DEFAULT 1,             -- 0 = disabled, skipped everywhere
    created_at    INTEGER NOT NULL,                       -- unix epoch ms (UTC)
    updated_at    INTEGER NOT NULL
);
