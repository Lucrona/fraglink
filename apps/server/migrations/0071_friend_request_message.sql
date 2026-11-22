-- Personal message sent with a friend request (optional; shown to the recipient).
ALTER TABLE friendships ADD COLUMN IF NOT EXISTS message text;
