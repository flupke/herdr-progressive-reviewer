// What the agent's turn said back to the reviewer's previous answer, above its question or
// conclusion, as the pane shows it: its recap of the answer and the follow-ups it recorded,
// then its reply.

/** @import { ResponseView } from "./types.ts" */

import { h, markdown } from './dom.js';

/** @param {ResponseView} response */
export function responseSection(response) {
  return h(
    'section',
    { class: 'response', 'aria-labelledby': 'response-title' },
    h('h2', { id: 'response-title' }, 'Reply from the agent'),
    response.interpretations.map((interpretation) => [
      interpretation.recap_html ? markdown(interpretation.recap_html, 'recap') : null,
      interpretation.follow_ups.length > 0
        ? h(
            'ul',
            { class: 'follow-ups' },
            interpretation.follow_ups.map((followUp) => h('li', {}, `Follow-up: ${followUp}`)),
          )
        : null,
    ]),
    response.reply_html ? markdown(response.reply_html) : null,
  );
}
