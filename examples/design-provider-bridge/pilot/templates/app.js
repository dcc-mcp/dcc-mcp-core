"use strict";

(() => {
  const original = JSON.parse(document.getElementById("pilot-document").textContent);
  let spec = JSON.parse(JSON.stringify(original));
  let selected = spec.page.assets[0].id;
  let activeFilter = "All assets";
  const cards = [...document.querySelectorAll("[data-asset-id]")];
  const title = document.getElementById("page-title");
  const titleInput = document.getElementById("title-input");
  const accentInput = document.getElementById("accent-input");
  const status = document.getElementById("edit-status");
  const search = document.getElementById("asset-search");
  const filters = [...document.querySelectorAll("[data-filter]")];

  function selectAsset(id) {
    const asset = spec.page.assets.find(item => item.id === id);
    if (!asset) return;
    selected = id;
    cards.forEach(card => card.setAttribute("aria-pressed", String(card.dataset.assetId === id)));
    document.getElementById("selected-name").textContent = asset.name;
    document.getElementById("selected-description").textContent = asset.description;
    document.getElementById("selected-category").textContent = asset.category;
    document.getElementById("selected-format").textContent = asset.format;
    document.getElementById("selected-items").textContent = asset.items;
  }

  function applyFilter() {
    const term = search.value.trim().toLowerCase();
    const visible = new Set(spec.page.assets.filter(asset =>
      (activeFilter === "All assets" || asset.category === activeFilter) &&
      `${asset.name} ${asset.category} ${asset.description}`.toLowerCase().includes(term)
    ).map(asset => asset.id));
    cards.forEach(card => { card.hidden = !visible.has(card.dataset.assetId); });
    document.getElementById("empty-state").hidden = visible.size !== 0;
    document.getElementById("result-count").textContent = `${visible.size} of ${cards.length} assets`;
    document.getElementById("inspector").hidden = visible.size === 0;
    if (visible.size && !visible.has(selected)) selectAsset(visible.values().next().value);
  }

  function applyAccent(value) {
    spec.tokens["color.accent"].value = value.toUpperCase();
    document.documentElement.style.setProperty("--color-accent", value);
    const channel = offset => parseInt(value.slice(offset, offset + 2), 16) / 255;
    const linear = x => x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    const luminance = 0.2126 * linear(channel(1)) + 0.7152 * linear(channel(3)) + 0.0722 * linear(channel(5));
    const ink = luminance > 0.179 ? "#000000" : "#FFFFFF";
    spec.tokens["color.accent-ink"].value = ink;
    document.documentElement.style.setProperty("--color-accent-ink", ink);
    document.getElementById("accent-value").textContent = value.toUpperCase();
  }

  cards.forEach(card => card.addEventListener("click", () => selectAsset(card.dataset.assetId)));
  filters.forEach(button => button.addEventListener("click", () => {
    activeFilter = button.dataset.filter;
    filters.forEach(item => item.setAttribute("aria-pressed", String(item === button)));
    applyFilter();
  }));
  search.addEventListener("input", applyFilter);
  titleInput.addEventListener("input", () => {
    const value = titleInput.value.trim();
    if (!value) {
      titleInput.setAttribute("aria-invalid", "true");
      status.textContent = "Enter a page title before exporting.";
      return;
    }
    titleInput.removeAttribute("aria-invalid");
    spec.page.title = value;
    title.textContent = value;
    status.textContent = "Page title updated. Export to keep your changes.";
  });
  accentInput.addEventListener("input", () => {
    applyAccent(accentInput.value);
    status.textContent = "Accent updated across the component library.";
  });
  document.getElementById("reset-edits").addEventListener("click", () => {
    spec = JSON.parse(JSON.stringify(original));
    titleInput.value = spec.page.title;
    titleInput.removeAttribute("aria-invalid");
    title.textContent = spec.page.title;
    accentInput.value = spec.tokens["color.accent"].value;
    applyAccent(accentInput.value);
    status.textContent = "Page edits reset.";
  });
  document.getElementById("export-document").addEventListener("click", () => {
    if (!titleInput.value.trim()) { titleInput.focus(); return; }
    const blob = new Blob([`${JSON.stringify(spec, null, 2)}\n`], {type: "application/json"});
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "fieldkit-edited.json";
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    status.textContent = "Editable page specification exported.";
  });
  selectAsset(selected);
  applyFilter();
})();
