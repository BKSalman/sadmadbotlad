import websocket

ws = websocket.WebSocket()

ws.connect("ws://localhost:3000")

ws.send("Alert { new: true, type: Raid { from: \"lmao\", viewers: 9999 } }")

print(ws.recv())
