#!/bin/zsh

ffmpeg -i $1 -acodec pcm_s16le -ar 16000 /tmp/whisper.wav
~/dev/whisper.cpp/whisper --model ~/dev/whisper.cpp/models/ggml-large-v3.bin -ovtt /tmp/whisper.wav -of ${1%.*}
new_file=`cat ${1%.*}.vtt | llm -s 'summarise this conversation in a filename without extension'`
cp $1 done/${new_file}.wav
cp ${1%.*}.vtt done/${new_file}.vtt
echo ${new_file}
rm /tmp/whisper.wav

