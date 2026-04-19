import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: 'com.example.hello' });

plugin.tool('greet', {
  description: 'Say hello.',
  run: async ({ input }) => {
    const who = input?.name ?? 'world';
    return `hello, ${who}`;
  },
});

plugin.run();
