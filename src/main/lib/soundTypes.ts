
export interface SoundSource {
  __modulePath: string;
  key: string;
  caption: string;
  press: {
      SPACE: string;
      ENTER: string;
      BACKSPACE: string;
      GENERICR0: string;
      GENERICR1: string;
      GENERICR2: string;
      GENERICR3: string;
      GENERICR4: string;
      [key: string]: string;
  };
  release: {
      SPACE: string;
      ENTER: string;
      BACKSPACE: string;
      GENERIC: string;
      [key: string]: string;
  };
}
