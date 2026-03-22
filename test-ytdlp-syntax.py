import json
import subprocess
import time
from sys import stdin, stdout
from typing import Any, Dict, List

from yt_dlp import YoutubeDL, parse_options

ydl = YoutubeDL({})

def test_ytdlp_syntax():
    ydl = YoutubeDL({})
    ydl.download("https://www.youtube.com/watch?v=dQw4w9WgXcQ")

def create_ytdlp_options(cliargs: List[str]) -> Dict[str, Any]:
    return dict(parse_options([]).ydl_opts).fromkeys(["quiet", "no_warnings", "overwrites", "postprocessors"], True)

def main():
    options = create_ytdlp_options(["-f", "bestvideo+bestaudio/best"])
    options.update({
        "postprocessors": [
            {
                "key": "Merger+ffmpeg_i1:-stream_loop -1",
                "when": "after_video_download",
            },
        ],
        "overwrites": True,
        "quiet": True,
        "no_warnings": True,
    })
    print(json.dumps({
      "id": "waaa40m90",
      "method": "download",
      "params": {
        "url": "https://coub.com/view/47bpc9",
        "options": json.dumps(options),
      },
    }) + "\n")



main()
