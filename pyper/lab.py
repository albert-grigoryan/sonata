import os
from pyper import Piper



os.environ["ORT_DYLIB_PATH"] = r"D:\onnxruntime_libs\x86\onnxruntime.dll"
os.environ["PIPER_ESPEAKNG_DATA_DIRECTORY"] = r"D:\projects\blindpandas\piper-rs\deps\windows\espeak-ng-build"


p = Piper(
    r"D:\Piper_TTS_Voices\voices\voice-en-us-amy-low\en-us-amy-low.onnx.json",
    r"D:\Piper_TTS_Voices\voices\voice-en-us-amy-low\en-us-amy-low.onnx"
)


text = "Who are you? said the Caterpillar. Replied Alice , rather shyly, I hardly know, sir!"
args = (text, None, None, None)
ret = p.synthesize_batched(*args, 3)
print(next(ret))

