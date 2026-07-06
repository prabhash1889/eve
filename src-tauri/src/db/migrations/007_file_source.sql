-- Phase C: file transcription. Transcripts produced from a dropped/picked audio
-- file record the source path (audio is read in place, never copied or stored).
-- NULL for ordinary microphone dictations.

ALTER TABLE transcripts ADD COLUMN source_file TEXT;
