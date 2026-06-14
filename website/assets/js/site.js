/* Duckle website behavior: theme toggle, GitHub star count, mobile nav. */
(function () {
    "use strict";

    var root = document.documentElement;

    /* ---- theme toggle (persisted; default dark, set pre-paint in <head>) ---- */
    var toggle = document.getElementById("themeToggle");
    if (toggle) {
        toggle.addEventListener("click", function () {
            var next = root.getAttribute("data-theme") === "light" ? "dark" : "light";
            root.setAttribute("data-theme", next);
            try { localStorage.setItem("duckle-theme", next); } catch (e) {}
        });
    }

    /* ---- mobile nav ---- */
    var navToggle = document.getElementById("navToggle");
    var navLinks = document.getElementById("navLinks");
    if (navToggle && navLinks) {
        navToggle.addEventListener("click", function () { navLinks.classList.toggle("open"); });
        navLinks.addEventListener("click", function (e) {
            if (e.target.tagName === "A") navLinks.classList.remove("open");
        });
    }

    /* ---- GitHub star count ----
       duckdb.org renders a static build-time count; we render a "★" fallback and
       upgrade it to the live number via the public API, cached for an hour so we
       do not hammer the rate limit on every page view. */
    var REPO = "SouravRoy-ETL/duckle";
    var countEl = document.getElementById("ghCount");
    function fmt(n) {
        if (n >= 1000) return (n / 1000).toFixed(n >= 10000 ? 0 : 1).replace(/\.0$/, "") + "k";
        return String(n);
    }
    function showStars(n) {
        if (countEl) countEl.textContent = "★ " + fmt(n);
    }
    if (countEl) {
        var cached = null;
        try { cached = JSON.parse(localStorage.getItem("duckle-stars") || "null"); } catch (e) {}
        var fresh = cached && (Date.now() - cached.t < 3600000);
        if (cached && typeof cached.n === "number") showStars(cached.n);
        if (!fresh) {
            fetch("https://api.github.com/repos/" + REPO, { headers: { Accept: "application/vnd.github+json" } })
                .then(function (r) { return r.ok ? r.json() : null; })
                .then(function (d) {
                    if (d && typeof d.stargazers_count === "number") {
                        showStars(d.stargazers_count);
                        try { localStorage.setItem("duckle-stars", JSON.stringify({ n: d.stargazers_count, t: Date.now() })); } catch (e) {}
                    }
                })
                .catch(function () { /* keep fallback */ });
        }
    }

    /* ---- docs sidebar: mark the current page active ---- */
    var here = location.pathname.split("/").pop() || "index.html";
    document.querySelectorAll(".docs-side a").forEach(function (a) {
        var href = (a.getAttribute("href") || "").split("/").pop();
        if (href === here) a.classList.add("active");
    });
})();
