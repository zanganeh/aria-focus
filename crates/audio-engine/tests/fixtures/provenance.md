# Audio-engine decoder/matrix fixtures

These binary fixtures are committed test assets. They were generated **once**
with FFmpeg 7.1 (`C:\ProgramData\chocolatey\bin\ffmpeg.exe`) on the development
machine and are checked in so the test suite never depends on FFmpeg at
runtime. Every file is a 1.000 s, 220 Hz sine (or `anullsrc` silence) so the
decoded duration is exactly `sample_rate` frames.

## Generation commands (run once, not at test time)

```sh
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=44100" -c:a pcm_s16le -ac 1 wav_pcm16_mono_44100.wav -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=48000" -c:a pcm_s24le -ac 2 wav_pcm24_stereo_48000.wav -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=96000" -c:a pcm_f32le -ac 2 wav_f32_stereo_96000.wav -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=44100" -c:a flac -sample_fmt s16 -ac 1 flac_mono_44100.flac -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=48000" -c:a flac -sample_fmt s16 -ac 2 flac_stereo_48000.flac -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=44100" -c:a libmp3lame -b:a 128k -ac 1 mp3_mono_44100.mp3 -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=48000" -c:a libmp3lame -b:a 128k -ac 2 mp3_stereo_48000.mp3 -y
ffmpeg -f lavfi -i "sine=frequency=220:duration=1:sample_rate=48000" -filter_complex "[0:a]pan=stereo|c0=c0|c1=c0[a]" -map "[a]" -c:a libopus -b:a 96k -vbr off -application audio -frame_duration 20 ogg_opus_stereo_48000.opus -y
ffmpeg -f lavfi -i "anullsrc=channel_layout=mono:sample_rate=44100" -t 1 -c:a pcm_s16le wav_zero_mono_44100.wav -y
```

## Matrix coverage

| file                       | codec                 | rate  | channels | bit depth  |
| -------------------------- | --------------------- | ----- | -------- | ---------- |
| wav_pcm16_mono_44100.wav   | WAV PCM s16le         | 44100 | mono     | 16         |
| wav_pcm24_stereo_48000.wav | WAV PCM s24le         | 48000 | stereo   | 24         |
| wav_f32_stereo_96000.wav   | WAV PCM f32le         | 96000 | stereo   | 32 (float) |
| flac_mono_44100.flac       | FLAC                  | 44100 | mono     | 16         |
| flac_stereo_48000.flac     | FLAC                  | 48000 | stereo   | 16         |
| mp3_mono_44100.mp3         | MP3 (libmp3lame)      | 44100 | mono     | n/a        |
| mp3_stereo_48000.mp3       | MP3 (libmp3lame)      | 48000 | stereo   | n/a        |
| ogg_opus_stereo_48000.opus | Ogg Opus (libopus)    | 48000 | stereo   | n/a        |
| wav_zero_mono_44100.wav    | WAV PCM s16le silence | 44100 | mono     | 16         |

Corrupt, truncated, and metadata-mismatch cases are derived in the test from
these committed files; no separate corrupt assets are stored.
