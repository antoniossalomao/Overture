PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS songs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    artist TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL DEFAULT '',
    group_id INTEGER REFERENCES groups(id) ON DELETE SET NULL,
    notes TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'manual',
    archived INTEGER NOT NULL DEFAULT 0,
    duplicate_marked INTEGER NOT NULL DEFAULT 0,
    normalized_hash TEXT NOT NULL DEFAULT '',
    search_blob TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_songs_archived_title ON songs(archived, title);
CREATE INDEX IF NOT EXISTS idx_songs_hash ON songs(normalized_hash);
CREATE INDEX IF NOT EXISTS idx_songs_title_artist ON songs(title, artist);

CREATE TABLE IF NOT EXISTS song_slides (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    "order" INTEGER NOT NULL,
    text TEXT NOT NULL,
    forced_break INTEGER NOT NULL DEFAULT 0,
    duration_secs INTEGER
);

CREATE INDEX IF NOT EXISTS idx_song_slides_song_order ON song_slides(song_id, "order");

CREATE TABLE IF NOT EXISTS themes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    is_return_theme INTEGER NOT NULL DEFAULT 0,
    json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS services (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,
    name TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_services_date ON services(archived, date DESC);

CREATE TABLE IF NOT EXISTS service_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    "order" INTEGER NOT NULL,
    kind TEXT NOT NULL,
    ref_id INTEGER,
    title TEXT NOT NULL DEFAULT '',
    subtitle TEXT NOT NULL DEFAULT '',
    params TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_service_items_service_order ON service_items(service_id, "order");

CREATE TABLE IF NOT EXISTS bible_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    abbr TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    lang TEXT NOT NULL,
    license TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS bible_verses (
    version_id INTEGER NOT NULL REFERENCES bible_versions(id) ON DELETE CASCADE,
    book TEXT NOT NULL,
    book_order INTEGER NOT NULL DEFAULT 1,
    chapter INTEGER NOT NULL,
    verse INTEGER NOT NULL,
    text TEXT NOT NULL,
    search_blob TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (version_id, book, chapter, verse)
);

CREATE INDEX IF NOT EXISTS idx_bible_verses_order ON bible_verses(version_id, book_order, chapter, verse);

CREATE TABLE IF NOT EXISTS bible_favorites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version_id INTEGER NOT NULL REFERENCES bible_versions(id) ON DELETE CASCADE,
    book TEXT NOT NULL,
    chapter INTEGER NOT NULL,
    verse INTEGER NOT NULL,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(version_id, book, chapter, verse)
);

CREATE TABLE IF NOT EXISTS bible_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version_id INTEGER NOT NULL REFERENCES bible_versions(id) ON DELETE CASCADE,
    book TEXT NOT NULL,
    chapter INTEGER NOT NULL,
    verse INTEGER NOT NULL,
    text TEXT NOT NULL,
    accessed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS media (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    duration REAL,
    archived INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS execution_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_item_id INTEGER REFERENCES service_items(id) ON DELETE SET NULL,
    song_id INTEGER REFERENCES songs(id) ON DELETE SET NULL,
    executed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_execution_log_date ON execution_log(executed_at);
CREATE INDEX IF NOT EXISTS idx_execution_log_song ON execution_log(song_id, executed_at);

CREATE TABLE IF NOT EXISTS team_members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    archived INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS schedules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    member_id INTEGER NOT NULL REFERENCES team_members(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    UNIQUE(service_id, member_id, role)
);

CREATE INDEX IF NOT EXISTS idx_schedules_member ON schedules(member_id);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
