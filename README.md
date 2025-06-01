# Parlor Room

A matchmaking sevice designed to support reasonable Bot and Player MMR queueing.

## Design

All queues are done into **lobbies**, which determine the composition of bots and players. Ideally, we would have two rooms:

- 100% Bot "Rapid Queuing": Used for bot performance ranking, leagues, etc.
- Mixed Player + Bot rooms: Allow players to play against each other, and supplement with bots to keep queue times low.

### Bot Queuing

Bot queuing for placements should be performed by an additional service. 