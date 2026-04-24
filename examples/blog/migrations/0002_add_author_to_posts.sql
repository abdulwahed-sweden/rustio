-- 0002: give posts an author column.
-- Existing rows default to 'anonymous' so the NOT NULL constraint is safe to add.
ALTER TABLE posts ADD COLUMN author TEXT NOT NULL DEFAULT 'anonymous';
CREATE INDEX posts_author_idx ON posts (author);
