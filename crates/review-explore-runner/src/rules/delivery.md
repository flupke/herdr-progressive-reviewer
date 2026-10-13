## Delivery

Each turn ends in one submit_question or submit_conclusion call. On a validation error, fix what
it reports and submit again; after a transport failure, retry the same arguments. A Previous
response error in `turn.md` repeats the error of a failed attempt. Once a call is accepted, end
your turn without a recap: the reviewer reads the turn on their screens, and the next answer
comes with a new index. If the tools are unavailable, say so and stop.
