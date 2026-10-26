-- Home page Quick Poll (0.3.0-beta.7 site). One vote per account per poll, changeable.
-- At most one active poll; past polls and their votes are kept. Additive only.
CREATE TABLE IF NOT EXISTS polls (
    id         bigserial PRIMARY KEY,
    question   text NOT NULL,
    active     boolean NOT NULL DEFAULT false,
    closed_at  timestamptz,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS polls_one_active ON polls (active) WHERE active;

CREATE TABLE IF NOT EXISTS poll_options (
    id      bigserial PRIMARY KEY,
    poll_id bigint NOT NULL REFERENCES polls(id) ON DELETE CASCADE,
    pos     int NOT NULL,
    label   text NOT NULL
);
CREATE INDEX IF NOT EXISTS poll_options_poll ON poll_options (poll_id, pos);

CREATE TABLE IF NOT EXISTS poll_votes (
    poll_id   bigint NOT NULL REFERENCES polls(id) ON DELETE CASCADE,
    user_id   uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    option_id bigint NOT NULL REFERENCES poll_options(id) ON DELETE CASCADE,
    voted_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (poll_id, user_id)
);
CREATE INDEX IF NOT EXISTS poll_votes_option ON poll_votes (poll_id, option_id);

-- First poll, zero votes (only if there are no polls yet).
DO $$
DECLARE pid bigint;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM polls) THEN
    INSERT INTO polls(question, active) VALUES ('Best LAN snack?', true) RETURNING id INTO pid;
    INSERT INTO poll_options(poll_id, pos, label) VALUES (pid, 1, 'Pizza'), (pid, 2, 'Energy drinks'), (pid, 3, 'Chips');
  END IF;
END $$;
