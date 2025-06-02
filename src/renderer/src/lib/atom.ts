import { createJSONStorage } from "jotai/utils";

export const tempStorage = createJSONStorage(() => sessionStorage);
