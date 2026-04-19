import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: 'com.example.social' });

plugin.tool('post_now', {
  description: 'Publish a content entity immediately.',
  inputSchema: {
    type: 'object',
    required: ['contentId'],
    properties: { contentId: { type: 'string' } },
  },
  run: async ({ input, core, log }) => {
    const content = await core.entity.get(input.contentId);
    if (!content) throw new Error('content not found');
    log(`posting ${content.id} to ${content.data.platform}`);
    // ... call the platform SDK here ...
    await core.entity.update(content.id, { ...content.data, status: 'posted' });
    return { id: content.id, status: 'posted' };
  },
});

plugin.run();
