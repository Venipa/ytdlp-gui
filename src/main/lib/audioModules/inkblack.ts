import path from "node:path";
import { parseAudioRoot } from "../audioPathParser";

const basePath = parseAudioRoot("blackink");

export default {
  key: "inkblack",
  caption: "Gateron Black Inks",
  press: {
    SPACE: path.join(basePath, "press", "SPACE.mp3"),
    ENTER: path.join(basePath, "press", "ENTER.mp3"),
    BACKSPACE: path.join(basePath, "press", "BACKSPACE.mp3"),
    GENERICR0: path.join(basePath, "press", "GENERIC_R0.mp3"),
    GENERICR1: path.join(basePath, "press", "GENERIC_R1.mp3"),
    GENERICR2: path.join(basePath, "press", "GENERIC_R2.mp3"),
    GENERICR3: path.join(basePath, "press", "GENERIC_R3.mp3"),
    GENERICR4: path.join(basePath, "press", "GENERIC_R4.mp3"),
  },
  release: {
    SPACE: path.join(basePath, "release", "SPACE.mp3"),
    ENTER: path.join(basePath, "release", "ENTER.mp3"),
    BACKSPACE: path.join(basePath, "release", "BACKSPACE.mp3"),
    GENERIC: path.join(basePath, "release", "GENERIC.mp3"),
  },
};
