---
hide:
  - toc
---

# BenchBear — Inventory, Locations, Suppliers &amp; Barcodes

<iframe id="inventory-hub-frame" src="http://127.0.0.1:8090/?embed=1" title="BenchBear Inventory Hub" style="width:100%;height:70vh;min-height:460px;border:1px solid rgba(140,145,155,0.22);border-radius:10px;background:#171a21;display:block;"></iframe>

<script>
(function () {
  var host = window.location.hostname || "127.0.0.1";
  var base = window.location.protocol + "//" + host + ":8090/";
  var frame = document.getElementById("inventory-hub-frame");
  if (!frame) return;
  if (frame.src !== base + "?embed=1") frame.src = base + "?embed=1";

  function resizeFrame() {
    var rect = frame.getBoundingClientRect();
    var avail = Math.max(460, Math.floor(window.innerHeight - rect.top - 18));
    frame.style.height = avail + "px";
  }

  resizeFrame();
  window.addEventListener("resize", resizeFrame);
  frame.addEventListener("load", resizeFrame);
  setTimeout(resizeFrame, 100);
  setTimeout(resizeFrame, 350);
})();
</script>
