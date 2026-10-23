/* Supervisor site behaviour. No dependencies, no network calls. */
(function () {
  "use strict";

  // Tells the inline head script that its fallback timer is not needed.
  document.documentElement.classList.add("site-ready");

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---- theme ---- */
  var root = document.documentElement;
  var themeButton = document.getElementById("theme-toggle");
  var systemDark = window.matchMedia("(prefers-color-scheme: dark)");

  function currentTheme() {
    return root.getAttribute("data-theme") === "dark" ? "dark" : "light";
  }

  function labelTheme() {
    if (!themeButton) return;
    var next = currentTheme() === "dark" ? "light" : "dark";
    themeButton.setAttribute("aria-label", "Switch to the " + next + " theme");
  }

  function setTheme(theme, remember) {
    if (theme === "dark") root.setAttribute("data-theme", "dark");
    else root.setAttribute("data-theme", "light");

    if (remember) {
      try {
        localStorage.setItem("supervisor-theme", theme);
      } catch (error) {
        /* storage blocked: the choice just will not survive a reload */
      }
    }
    labelTheme();
  }

  labelTheme();

  if (themeButton) {
    themeButton.addEventListener("click", function () {
      setTheme(currentTheme() === "dark" ? "light" : "dark", true);
    });
  }

  // Follow the OS until the visitor picks a side themselves.
  systemDark.addEventListener("change", function (event) {
    var chosen = null;
    try {
      chosen = localStorage.getItem("supervisor-theme");
    } catch (error) {
      /* ignore */
    }
    if (chosen !== "dark" && chosen !== "light") setTheme(event.matches ? "dark" : "light", false);
  });

  /* ---- reveal on scroll ---- */
  var revealables = document.querySelectorAll(".reveal");

  if (reduceMotion || !("IntersectionObserver" in window)) {
    revealables.forEach(function (el) {
      el.classList.add("in");
    });
  } else {
    var revealer = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          // Stagger siblings so a row of cards arrives in sequence.
          var siblings = Array.prototype.slice.call(entry.target.parentNode.children);
          var step = Math.min(siblings.indexOf(entry.target), 4);
          entry.target.style.transitionDelay = step * 70 + "ms";
          entry.target.classList.add("in");
          revealer.unobserve(entry.target);
        });
      },
      { rootMargin: "0px 0px -12% 0px", threshold: 0.08 }
    );

    revealables.forEach(function (el) {
      revealer.observe(el);
    });
  }

  /* ---- highlight the nav link for the section on screen ---- */
  var navLinks = Array.prototype.slice.call(document.querySelectorAll(".site-nav a"));
  var sections = navLinks
    .map(function (link) {
      return document.querySelector(link.getAttribute("href"));
    })
    .filter(Boolean);

  if (sections.length) {
    var header = document.querySelector(".site-header");
    var queued = false;

    var syncNav = function () {
      queued = false;

      // The last section whose top has passed just under the sticky header wins.
      var line = (header ? header.getBoundingClientRect().height : 0) + 90;
      var winner = null;

      sections.forEach(function (section) {
        if (section.getBoundingClientRect().top <= line) winner = section.id;
      });

      // The final section is often too short to ever cross the line on its own.
      if (window.innerHeight + window.scrollY >= document.body.scrollHeight - 2) {
        winner = sections[sections.length - 1].id;
      }

      navLinks.forEach(function (link) {
        var on = winner !== null && link.getAttribute("href") === "#" + winner;
        link.classList.toggle("current", on);
        if (on) link.setAttribute("aria-current", "true");
        else link.removeAttribute("aria-current");
      });
    };

    var queueSync = function () {
      if (queued) return;
      queued = true;
      window.requestAnimationFrame(syncNav);
    };

    window.addEventListener("scroll", queueSync, { passive: true });
    window.addEventListener("resize", queueSync);
    syncNav();
  }

  /* ---- state preview on the hero mock ---- */
  var picker = document.getElementById("state-picker");
  var slab = document.getElementById("mock-slab");
  if (!picker || !slab) return;

  var STATES = {
    focused: {
      className: "is-focused",
      status: "Focused",
      strictness: "Strict",
      title: "Locked in",
      copy: "Finish the session storage migration.",
      progress: 58,
      elapsed: "Elapsed 34:12",
      nudges: "Nudges 0"
    },
    watch: {
      className: "",
      status: "Distraction open",
      strictness: "Normal",
      title: "Distraction open",
      copy: "Two minutes on YouTube.",
      progress: 61,
      elapsed: "Elapsed 36:40",
      nudges: "Nudges 0"
    },
    drifting: {
      className: "is-drifting",
      status: "Distracted",
      strictness: "Normal",
      title: "Distracted",
      copy: "Six minutes on YouTube. Playing a nudge in Mum.",
      progress: 66,
      elapsed: "Elapsed 39:05",
      nudges: "Nudges 1"
    },
    away: {
      className: "is-away",
      status: "Away",
      strictness: "Normal",
      title: "Away",
      copy: "No keyboard or mouse for four minutes. Nudges paused.",
      progress: 66,
      elapsed: "Elapsed 39:05",
      nudges: "Nudges 1"
    }
  };

  var fields = {
    status: document.getElementById("mock-status"),
    strictness: document.getElementById("mock-strict"),
    title: document.getElementById("mock-title"),
    copy: document.getElementById("mock-copy"),
    bar: document.getElementById("mock-bar"),
    elapsed: document.getElementById("mock-elapsed"),
    nudges: document.getElementById("mock-nudges")
  };

  var buttons = Array.prototype.slice.call(picker.querySelectorAll("button"));

  function apply(key) {
    var state = STATES[key];
    if (!state) return;

    slab.className = "mock-slab " + state.className;
    fields.status.textContent = state.status;
    fields.strictness.textContent = state.strictness;
    fields.title.textContent = state.title;
    fields.copy.textContent = state.copy;
    fields.bar.style.width = state.progress + "%";
    fields.elapsed.textContent = state.elapsed;
    fields.nudges.textContent = state.nudges;

    buttons.forEach(function (button) {
      button.setAttribute("aria-pressed", String(button.dataset.state === key));
    });
  }

  buttons.forEach(function (button) {
    button.addEventListener("click", function () {
      apply(button.dataset.state);
    });
  });
})();
