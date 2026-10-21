// Tests run in English with British formats, whatever the computer's language is.
Object.defineProperty(globalThis.navigator, 'languages', { value: ['en-GB'], configurable: true });
Object.defineProperty(globalThis.navigator, 'language', { value: 'en-GB', configurable: true });
