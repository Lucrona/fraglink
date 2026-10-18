-- Sign-up now asks "I'm 13 or older (16 or older in the EU/UK)". When it was ticked;
-- NULL for accounts made before the tick box existed.
ALTER TABLE users ADD COLUMN age_confirmed_at timestamptz;
