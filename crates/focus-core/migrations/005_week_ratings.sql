-- The weekly check-in: "was this week well spent?" (1 to 5), the success
-- signal of the whole project. `week` is the ISO week, e.g. 2026-W43.
CREATE TABLE week_ratings (
    week     TEXT PRIMARY KEY,
    rating   INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 5),
    rated_at INTEGER NOT NULL
);
