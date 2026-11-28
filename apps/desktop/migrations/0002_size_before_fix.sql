-- the file's size before its first fix (ffmpeg after download), to show what the fix changed
ALTER TABLE downloads ADD COLUMN size_before_fix INTEGER;
