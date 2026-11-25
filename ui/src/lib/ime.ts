/**
 * Whether a key event is an input method confirming a candidate rather than
 * the user pressing the key. Chrome and Firefox set `isComposing`; Safari
 * reports the confirming Enter with `keyCode` 229 and clears `isComposing`
 * first, so both signals are checked.
 */
export interface ImeKeyEvent {
  nativeEvent?: { isComposing?: boolean };
  isComposing?: boolean;
  keyCode?: number;
}

export function isImeComposing(event: ImeKeyEvent): boolean {
  return event.nativeEvent?.isComposing === true
    || event.isComposing === true
    || event.keyCode === 229;
}
