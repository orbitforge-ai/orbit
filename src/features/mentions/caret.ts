const MIRROR_PROPS: (keyof CSSStyleDeclaration)[] = [
  'boxSizing',
  'width',
  'height',
  'overflowX',
  'overflowY',
  'borderTopWidth',
  'borderRightWidth',
  'borderBottomWidth',
  'borderLeftWidth',
  'borderStyle',
  'paddingTop',
  'paddingRight',
  'paddingBottom',
  'paddingLeft',
  'fontStyle',
  'fontVariant',
  'fontWeight',
  'fontStretch',
  'fontSize',
  'fontSizeAdjust',
  'lineHeight',
  'fontFamily',
  'textAlign',
  'textTransform',
  'textIndent',
  'textDecoration',
  'letterSpacing',
  'wordSpacing',
  'tabSize',
  'whiteSpace',
  'wordWrap',
  'wordBreak',
];

export interface CaretCoords {
  left: number;
  top: number;
  lineHeight: number;
}

export function getCaretCoords(textarea: HTMLTextAreaElement): CaretCoords {
  const rect = textarea.getBoundingClientRect();
  const computed = window.getComputedStyle(textarea);
  const mirror = document.createElement('div');
  const style = mirror.style;
  style.position = 'absolute';
  style.visibility = 'hidden';
  style.top = '0';
  style.left = '-9999px';
  for (const prop of MIRROR_PROPS) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (style as any)[prop] = (computed as any)[prop];
  }
  style.whiteSpace = 'pre-wrap';
  style.wordWrap = 'break-word';
  style.overflow = 'hidden';

  const selectionStart = textarea.selectionStart ?? textarea.value.length;
  mirror.textContent = textarea.value.slice(0, selectionStart);
  const marker = document.createElement('span');
  marker.textContent = '\u200b';
  mirror.appendChild(marker);
  document.body.appendChild(mirror);

  const markerRect = marker.getBoundingClientRect();
  const mirrorRect = mirror.getBoundingClientRect();

  const lineHeight = parseFloat(computed.lineHeight) || parseFloat(computed.fontSize) * 1.2;

  const left = rect.left + (markerRect.left - mirrorRect.left) - textarea.scrollLeft;
  const top = rect.top + (markerRect.top - mirrorRect.top) - textarea.scrollTop;

  document.body.removeChild(mirror);

  return { left, top, lineHeight };
}
