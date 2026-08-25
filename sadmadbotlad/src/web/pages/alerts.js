const bus = document.getElementById("alert-bus");
let seq = 0;

function connect() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(`${proto}//${location.host}/alerts/ws`);
  ws.onmessage = (e) => {
    // The seq prefix makes the shard argument change even when two
    // consecutive alerts are byte-identical.
    bus.value = `${seq++}:${e.data}`;
    bus.dispatchEvent(new Event("input"));
  };
  // OBS browser sources are long-lived; the socket must survive a bot restart.
  ws.onclose = () => setTimeout(connect, 1000);
  ws.onerror = () => ws.close();
}

connect();
