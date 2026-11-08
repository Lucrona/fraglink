-- Admin sections (Nov 2026): changelog entries staff write in Admin > Changelog. Published
-- ones are served by GET /api/v1/changelog and shown on /changelog next to the entries that
-- ship with the site. Drafts (published_at NULL) are staff-only. Every change is audited in
-- admin_audit (changelog.*). Download page links live in site_settings.value (no change here).
CREATE TABLE changelog_entries (
    id           bigserial PRIMARY KEY,
    area         text NOT NULL CHECK (area IN ('App', 'Website')),
    entry_date   date NOT NULL,
    version      text CHECK (version IS NULL OR length(version) BETWEEN 1 AND 40),
    new_items    text[] NOT NULL DEFAULT '{}',
    changes      text[] NOT NULL DEFAULT '{}',
    fixes        text[] NOT NULL DEFAULT '{}',
    published_at timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    created_by   uuid REFERENCES users(id) ON DELETE SET NULL,
    updated_at   timestamptz NOT NULL DEFAULT now(),
    updated_by   uuid REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX changelog_entries_published ON changelog_entries (entry_date DESC, id DESC)
    WHERE published_at IS NOT NULL;
CREATE INDEX changelog_entries_recent ON changelog_entries (updated_at DESC, id DESC);
