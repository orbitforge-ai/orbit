/**
 * Helper for declaring input schemas with TypeScript type inference. Used by
 * plugin authors when defining a tool:
 *
 * ```ts
 * const inputSchema = defineSchema({
 *   type: 'object',
 *   required: ['contentId'],
 *   properties: { contentId: { type: 'string' } }
 * });
 * ```
 */
export function defineSchema<T>(schema: T): T {
  return schema;
}
