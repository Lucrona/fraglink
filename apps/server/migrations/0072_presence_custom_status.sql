-- Custom status line friends can see (set from the desktop app).
ALTER TABLE presence ADD COLUMN IF NOT EXISTS custom_status text;
