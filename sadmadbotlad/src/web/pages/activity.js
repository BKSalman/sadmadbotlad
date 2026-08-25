const bus = document.getElementById("activity-bus");
let seq = 0;

function connect() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(`${proto}//${location.host}/alerts/ws`);
  ws.onmessage = () => {
    // The payload is ignored: the shard re-queries the database, which is the
    // source of truth. This only has to make the shard argument change.
    bus.value = String(seq++);
    bus.dispatchEvent(new Event("input"));
  };
  ws.onclose = () => setTimeout(connect, 1000);
  ws.onerror = () => ws.close();
}

connect();
