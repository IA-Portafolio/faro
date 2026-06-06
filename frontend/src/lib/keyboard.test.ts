import { describe, expect, it } from 'vitest';

import { isTyping } from './keyboard';

// `isTyping` only reads `.isContentEditable`, `.tagName` and `.closest(...)`
// off its target, so plain objects shaped like an element are enough to
// exercise every branch without a real DOM.
function el(props: Partial<HTMLElement> & { closestSelector?: string | null }): EventTarget {
  const { closestSelector, ...rest } = props;
  return {
    ...rest,
    closest: (sel: string) => (closestSelector && sel === closestSelector ? ({} as Element) : null)
  } as unknown as EventTarget;
}

describe('isTyping', () => {
  it('returns false for a null target', () => {
    expect(isTyping(null)).toBe(false);
  });

  it('returns true for contenteditable elements', () => {
    expect(isTyping(el({ isContentEditable: true, tagName: 'DIV' }))).toBe(true);
  });

  it('returns true for INPUT, TEXTAREA and SELECT', () => {
    expect(isTyping(el({ tagName: 'INPUT' }))).toBe(true);
    expect(isTyping(el({ tagName: 'TEXTAREA' }))).toBe(true);
    expect(isTyping(el({ tagName: 'SELECT' }))).toBe(true);
  });

  it('returns true inside a role="textbox" container', () => {
    expect(isTyping(el({ tagName: 'SPAN', closestSelector: '[role="textbox"]' }))).toBe(true);
  });

  it('returns false for a plain non-editable element', () => {
    expect(isTyping(el({ tagName: 'DIV', isContentEditable: false }))).toBe(false);
    expect(isTyping(el({ tagName: 'BUTTON' }))).toBe(false);
  });

  it('does not throw when the target lacks a closest() method', () => {
    const bare = { tagName: 'DIV' } as unknown as EventTarget;
    expect(() => isTyping(bare)).not.toThrow();
    expect(isTyping(bare)).toBe(false);
  });
});
