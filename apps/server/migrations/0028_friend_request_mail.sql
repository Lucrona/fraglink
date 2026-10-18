-- Friend-request email log (at most one mail per requester→target) and one-click
-- "turn off these emails" tokens (email_tokens.purpose = notify_off).
ALTER TABLE email_tokens DROP CONSTRAINT IF EXISTS email_tokens_purpose_check;
ALTER TABLE email_tokens ADD CONSTRAINT email_tokens_purpose_check
  CHECK (purpose IN ('verify', 'reset', 'notify_off'));

CREATE TABLE IF NOT EXISTS friend_request_mail_log (
    requester  uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sent_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (requester, target)
);
CREATE INDEX IF NOT EXISTS friend_request_mail_log_target_day
  ON friend_request_mail_log (target, sent_at DESC);
