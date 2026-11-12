Test media for the TV demuxer, generated with ffmpeg (synthetic test patterns and tones):
  ffmpeg -f lavfi -i testsrc2=size=320x180:rate=50 -f lavfi -i sine=frequency=440:sample_rate=48000 -t 1 -c:v libx264 -profile:v high -pix_fmt yuv420p -g 25 -bf 2 -c:a aac -ac 2 -b:a 96k -f mpegts h264_high_50fps.ts
  ffmpeg -i h264_high_50fps.ts -c:v copy -an -f h264 h264_high_50fps.h264
  ffmpeg -f lavfi -i testsrc2=size=320x180:rate=25 -f lavfi -i sine=frequency=330:sample_rate=48000 -t 1 -c:v libx265 -pix_fmt yuv420p -c:a aac -ac 2 -f mpegts h265.ts
  ffmpeg -f lavfi -i "color=black:s=320x176:r=25:d=2[bg];color=white:s=16x16:d=2[b];[bg][b]overlay=x='mod(n*4\,300)':y=80" -c:v libx264 -profile:v high -pix_fmt yuv420p -g 25 -bf 3 -f mpegts box_bframes.ts   (a box moving 4 px per frame, with B-frames)
  ffmpeg -f lavfi -i testsrc2=size=160x90:rate=25 -f lavfi -i sine=frequency=440:sample_rate=48000 -t 2 -c:v libx264 -profile:v high -pix_fmt yuv420p -g 25 -bf 0 -c:a aac -ac 2 -b:a 192k -pes_payload_size 8000 -f mpegts audio_big_chunks.ts   (sound in ~333 ms chunks, like many IPTV channels)
