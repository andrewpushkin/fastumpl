#!/usr/bin/env python3
"""Writes the big diagrams of tests/big/, used to test how large and extreme
inputs are drawn (see docs/qa/2026-11-20-big-session.md).

    scripts/dev.sh python3 scripts/big-files.py     # rewrites tests/big/

Each file pushes one dimension: the count of elements, the length of text,
the size of coordinates, or the size of numbers. The output is the same on
every run (fixed seed), so the files can be regenerated and diffed.
"""

import os
import random

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "tests", "big")

WORDS = (
    "order invoice payment customer account ledger entry shipment parcel "
    "route depot vehicle driver tariff discount coupon basket product price "
    "stock warehouse supplier contract audit report session token user role "
    "permission tenant region zone queue event handler policy rule schedule"
).split()


def camel(rng, n=2):
    return "".join(rng.choice(WORDS).capitalize() for _ in range(n))


def write(name, text):
    path = os.path.join(OUT, name)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"{name}: {len(text.splitlines())} lines, {len(text)} bytes")


# --------------------------------------------------------------------------
# Class diagrams (.umpl)


def class_many(classes=220, relations=460, seed=1):
    """Many elements: above the 150-class background layout threshold, in
    packages two levels deep, with every relation kind."""
    rng = random.Random(seed)
    packages = [
        "core", "core.model", "core.io", "billing", "shipping", "catalog",
        "identity", "reporting", "audit", "pricing", "inventory", "notify",
    ]
    # Spread the classes over the packages and the top level.
    names = set()
    elements = []  # (package or None, name, kind)
    while len(elements) < classes:
        name = camel(rng, rng.choice([2, 2, 3]))
        if name in names:
            continue
        names.add(name)
        pkg = rng.choice(packages + [None, None])
        kind = rng.choices(
            ["class", "abstract class", "interface", "enum"], [62, 12, 14, 8]
        )[0]
        elements.append((pkg, name, kind))

    def qual(e):
        return f"{e[0]}.{e[1]}" if e[0] else e[1]

    def body(kind):
        if kind == "enum":
            lits = [rng.choice(WORDS).upper() for _ in range(rng.randint(2, 7))]
            return " { " + ", ".join(dict.fromkeys(lits)) + " }"
        n = rng.choice([0, 0, 1, 2, 3, 4, 6, 9])
        if n == 0:
            return ""
        lines = []
        for _ in range(n):
            vis = rng.choice("+-#~")
            if rng.random() < 0.5:
                lines.append(f"  {vis}{rng.choice(WORDS)}: {camel(rng, 1)}")
            else:
                args = ", ".join(
                    f"{rng.choice(WORDS)}: {camel(rng, 1)}"
                    for _ in range(rng.randint(0, 3))
                )
                lines.append(f"  {vis}{rng.choice(WORDS)}({args}): {camel(rng, 1)}")
        return " {\n" + "\n".join(lines) + "\n}"

    out = [f'diagram "Many classes: {classes} elements, {relations} relations"', ""]

    def emit(pkg, indent):
        pad = "  " * indent
        for e in elements:
            if e[0] == pkg:
                text = f"{e[2]} {e[1]}{body(e[2])}"
                out.extend(pad + line for line in text.split("\n"))
        for child in packages:
            parent, _, leaf = child.rpartition(".")
            if (parent or None) == pkg:
                out.append(f"{pad}package {leaf} {{")
                emit(child, indent + 1)
                out.append(f"{pad}}}")

    emit(None, 0)
    out.append("")

    seen = set()
    classes_only = [e for e in elements if e[2] != "enum"]
    interfaces = [e for e in elements if e[2] == "interface"]
    arrows = ["--|>", "..|>", "*--", "o--", "--", "-->", "..>"]
    while len(seen) < relations:
        a = rng.choice(classes_only)
        arrow = rng.choices(arrows, [18, 10, 14, 10, 14, 22, 12])[0]
        # Mostly inside a package, so the drawing has structure.
        same = [e for e in elements if e[0] == a[0] and e != a]
        b = rng.choice(same if same and rng.random() < 0.6 else elements)
        if arrow == "..|>" and interfaces:
            b = rng.choice(interfaces)
        if b == a or (qual(a), qual(b)) in seen or (qual(b), qual(a)) in seen:
            continue
        seen.add((qual(a), qual(b)))
        left = right = label = ""
        if arrow in ("*--", "o--", "--") and rng.random() < 0.7:
            left = f' "{rng.choice(["1", "0..1", "*"])}"'
            right = f' "{rng.choice(["1", "1..*", "*", "0..*"])}"'
        if rng.random() < 0.3:
            label = f" : {rng.choice(WORDS)}"
        out.append(f"{qual(a)}{left} {arrow}{right} {qual(b)}{label}")

    for i in range(6):
        out.append(f'note N{i} "Note {i}: {" ".join(rng.choice(WORDS) for _ in range(8))}"')
        out.append(f"N{i} .. {qual(rng.choice(elements))}")
    return "\n".join(out) + "\n"


def class_huge(classes=1500, relations=3000, seed=2):
    """Very many elements, past what the router routes (draft lines)."""
    rng = random.Random(seed)
    out = ['diagram "Huge: 1500 classes, 3000 relations"', ""]
    names = [f"C{i:04d}{camel(rng, 1)}" for i in range(classes)]
    for i, n in enumerate(names):
        if i % 50 == 0:
            if i:
                out.append("}")
            out.append(f"package p{i // 50:02d} {{")
        members = "".join(
            f"\n    +{rng.choice(WORDS)}: {camel(rng, 1)}" for _ in range(rng.randint(0, 3))
        )
        out.append(f"  class {n}" + (f" {{{members}\n  }}" if members else ""))
    out.append("}")
    seen = set()
    while len(seen) < relations:
        i = rng.randrange(classes)
        j = (i + rng.randint(1, 80)) % classes if rng.random() < 0.85 else rng.randrange(classes)
        if i == j or (i, j) in seen:
            continue
        seen.add((i, j))
        a = f"p{i // 50:02d}.{names[i]}"
        b = f"p{j // 50:02d}.{names[j]}"
        out.append(f"{a} {rng.choice(['-->', '--|>', '*--', '..>'])} {b}")
    return "\n".join(out) + "\n"


def class_long_text():
    """Long names, members, labels and notes; many members and literals;
    wide (CJK, emoji) and combining characters."""
    long_name = "VeryLongClassName" + "".join(f"Part{i:02d}" for i in range(26))  # ~120
    out = ['diagram "' + "A diagram title that is much longer than the window is wide, " * 4 + 'the end"', ""]
    out.append(f"class {long_name} {{")
    out.append("  +" + "x" * 400 + ": String")
    out.append("  +f(" + ", ".join(f"arg{i}: Map<String, List<Option<Vec<u8>>>>" for i in range(12)) + "): Result<(), Box<dyn Error + Send + Sync + 'static>>")
    out.append("}")
    out.append("class TwoHundredMembers {")
    for i in range(200):
        out.append(f"  +member{i:03d}: Type{i:03d}")
    out.append("}")
    out.append("class ManyMethods {")
    for i in range(80):
        out.append(f"  +method{i:02d}(a: Int, b: Int): Int")
    out.append("}")
    lits = ", ".join(f"LITERAL_{i:03d}" for i in range(300))
    out.append(f"enum ThreeHundredLiterals {{ {lits} }}")
    out.append("class 注文管理システム {")
    out.append("  +顧客名: 文字列")
    out.append("  +合計金額(): 金額")
    out.append("}")
    # Emoji are not letters, so not part of a name; members take them.
    out.append("class EmojiRocket {")
    out.append("  +status: 🟢🟡🔴")
    out.append("}")
    out.append("class Combining {")
    out.append("  +naïve: Café")
    out.append("  +z̷̢̛a̸l̵g̶o̴: String")
    out.append("}")
    out.append("package " + "a_package_with_a_very_long_name_" * 4 + "end {")
    out.append("  class Inner")
    out.append("  class " + "Deep" * 30)
    out.append("}")
    for i in range(6):
        out.append(f"class Small{i}")
    out.append("")
    pkg = "a_package_with_a_very_long_name_" * 4 + "end"
    out.append(f'{long_name} "0..2147483647" *-- "1..9223372036854775807 a_very_long_role_name_for_this_end" TwoHundredMembers : ' + "composed of, " * 15 + "end")
    out.append("TwoHundredMembers --> ManyMethods : " + "\\n".join(f"line {i} of a label written over many lines" for i in range(10)))
    out.append(f"ManyMethods ..> ThreeHundredLiterals : uses")
    out.append(f"注文管理システム --> EmojiRocket : 打ち上げ 🚀")
    out.append(f"Combining --|> {long_name}")
    out.append(f"{pkg}.Inner --> {pkg}.{'Deep' * 30} : " + "deep " * 20)
    for i in range(6):
        out.append(f'Small{i} "{"9" * (20 + i * 10)}" -- "{"*" * 50}" Small{(i + 1) % 6}')
    note = "\\n".join(f"Note line {i}: " + "words " * 12 for i in range(40))
    out.append(f'note Tall "{note}"')
    out.append(f"Tall .. TwoHundredMembers")
    out.append('note Wide "' + "a single very long line of note text " * 30 + '"')
    out.append(f"Wide .. {long_name}")
    return "\n".join(out) + "\n"


def class_far_coords():
    """Positions at and past the limits (|x| <= 1,000,000), sizes at and past
    10,000, and lines between boxes a million units apart."""
    out = ['diagram "Far apart: positions at the 1,000,000 limit"', ""]
    names = [
        "Origin", "EastLimit", "WestLimit", "SouthLimit", "NorthLimit",
        "FarCorner", "NegCorner", "BeyondEast", "BeyondSouth",
        "HugeBox", "TooHuge", "Neighbour",
    ]
    for n in names:
        out.append(f"class {n} {{\n  +x: Int\n}}")
    out.append("")
    out.append("Origin --> EastLimit : one million east")
    out.append("Origin --> WestLimit")
    out.append("Origin --> SouthLimit : one million south")
    out.append("Origin --> NorthLimit")
    out.append("EastLimit --> FarCorner")
    out.append("WestLimit --> NegCorner")
    out.append("FarCorner ..> NegCorner : across the whole plane")
    out.append("BeyondEast --> Origin")
    out.append("BeyondSouth --> Origin")
    out.append("HugeBox --> Neighbour")
    out.append("TooHuge --> Neighbour")
    out.append("")
    out.append("layout {")
    out.append("  Origin      at 0, 0")
    out.append("  EastLimit   at 1000000, 0")
    out.append("  WestLimit   at -1000000, 0")
    out.append("  SouthLimit  at 0, 1000000")
    out.append("  NorthLimit  at 0, -1000000")
    out.append("  FarCorner   at 1000000, 1000000")
    out.append("  NegCorner   at -1000000, -1000000")
    out.append("  BeyondEast  at 1000001, 0")
    out.append("  BeyondSouth at 0, 2147483648")
    out.append("  HugeBox     at 40, 40 size 10000, 10000")
    out.append("  TooHuge     at 40, -40 size 10001, 10")
    out.append("  Neighbour   at 60, 0")
    out.append("}")
    return "\n".join(out) + "\n"


# --------------------------------------------------------------------------
# Sequence diagrams (PlantUML)

KINDS = ["participant", "actor", "boundary", "control", "entity", "database", "collections", "queue"]


def seq_many(participants=40, messages=1500, seed=3):
    """Many participants and messages, every arrow kind, fragments nested,
    activations, notes, dividers, delays, refs and boxes."""
    rng = random.Random(seed)
    names = [f"P{i:02d}{camel(rng, 1)}" for i in range(participants)]
    out = ["@startuml", "title Many: 40 participants, 1500 messages", "autonumber"]
    for i, n in enumerate(names):
        if i % 10 == 0:
            out.append(f'box "Group {i // 10}" #f0f0ff')
        out.append(f"{KINDS[i % len(KINDS)]} {n}")
        if i % 10 == 9:
            out.append("end box")
    # `-\`, `-/`, `o->` are left out: they make the file a class diagram
    # (QA-28, see seq-arrows.puml).
    arrows = ["->", "-->", "->>", "->x", "<->", "-[#red]>", "->o", "x->", "<--"]
    depth = 0
    active = []
    count = 0
    while count < messages:
        r = rng.random()
        if r < 0.03 and depth < 6:
            kw = rng.choice(["alt", "loop", "opt", "par", "group", "critical", "break"])
            out.append(f"{kw} {kw} case {count}")
            depth += 1
            continue
        if r < 0.05 and depth > 0:
            if rng.random() < 0.5:
                out.append(f"else other case {count}")
            else:
                out.append("end")
                depth -= 1
            continue
        if r < 0.055:
            out.append(f"== Section {count} ==")
            continue
        if r < 0.06:
            out.append(f"... {count} ms later ...")
            continue
        if r < 0.07:
            who = rng.choice(names)
            side = rng.choice(["left of", "right of", "over"])
            out.append(f"note {side} {who} : note at message {count}")
            continue
        if r < 0.075:
            a, b = sorted(rng.sample(range(participants), 2))
            out.append(f"ref over {names[a]}, {names[b]} : reference {count}")
            continue
        a = rng.choice(names)
        b = a if rng.random() < 0.05 else rng.choice(names)
        arrow = rng.choice(arrows)
        suffix = ""
        if rng.random() < 0.15 and b not in active:
            suffix = " ++"
            active.append(b)
        elif active and rng.random() < 0.15:
            b = active.pop()
            suffix = " --"
        out.append(f"{a} {arrow} {b}{suffix} : {rng.choice(WORDS)} {rng.choice(WORDS)}({count})")
        count += 1
        if count % 100 == 0:
            out.append(f"[-> {rng.choice(names)} : from the edge {count}")
            out.append(f"{rng.choice(names)} ->] : to the edge {count}")
    while depth:
        out.append("end")
        depth -= 1
    out.append("@enduml")
    return "\n".join(out) + "\n"


def seq_long_text():
    """Long and many-line names, labels and notes; fragments nested 30 deep;
    a ref over every participant; wide characters."""
    long = "A_participant_with_a_very_long_name_" * 4
    out = ["@startuml", "title " + "A sequence title far wider than the window " * 5]
    out.append(f'participant "{long}" as L')
    out.append('participant "' + "\\n".join(f"Name line {i}" for i in range(8)) + '" as M')
    out.append('actor "利用者 👩‍💻" as U')
    out.append('database "Données naïves" as D')
    out.append("participant S")
    out.append("U -> L : " + "a message label that goes on and on " * 14)
    out.append("L -> L : " + "a self call whose label is very long " * 8)
    out.append("L -> M : " + "\\n".join(f"label line {i}" for i in range(25)))
    out.append("M -> S : short")
    out.append("S -> U : " + "back across every lifeline " * 6)
    out.append("note over L, S\n" + "\n".join(f"note line {i}: " + "text " * 20 for i in range(30)) + "\nend note")
    out.append("note left of U : " + "a left note that is wide " * 10)
    out.append("note right of S : " + "a right note that is wide " * 10)
    for i in range(30):
        out.append("  " * i + f"{['alt', 'loop', 'opt', 'group'][i % 4]} level {i} " + "condition " * 3)
    out.append("  " * 30 + "U -> D : deep inside thirty fragments")
    for i in reversed(range(30)):
        if i % 4 == 0 and i:
            out.append("  " * i + "else the other branch at level %d" % i)
            out.append("  " * i + "  D -> U : else branch")
        out.append("  " * i + "end")
    out.append("== " + "a divider with a long text " * 8 + "==")
    out.append("... " + "a delay with a long text " * 8 + "...")
    out.append("ref over U, S : " + "a reference over every participant " * 6)
    out.append("U -> D : 日本語のメッセージ、とても長いラベル" * 3)
    out.append("@enduml")
    return "\n".join(out) + "\n"


def seq_big_numbers():
    """autonumber at the edge of 64-bit integers, steps that overflow,
    a number too big to read, long formats, and large spaces."""
    out = ["@startuml", "title Big numbers", "participant A", "participant B"]
    out.append("autonumber 9223372036854775806")
    out += ["A -> B : max - 1", "B -> A : max", "A -> B : max + 1 (overflows)"]
    out.append("autonumber 1 9223372036854775807")
    out += ["A -> B : 1", "B -> A : step = max", "A -> B : step overflow"]
    # `autonumber -N` is left out: PlantUML reads such a file as a class
    # diagram, and so does fastumpl.
    out.append("autonumber 10 -9223372036854775807")
    out += ["A -> B : 10", "B -> A : 10 - max", "A -> B : underflows"]
    out.append("autonumber 99999999999999999999999")
    out += ["A -> B : start too big for 64 bits"]
    out.append('autonumber 1 1 "<b>[' + "0" * 40 + ']"')
    out += ["A -> B : forty zeros"]
    out.append("autonumber 2147483646")
    out += ["A -> B : int max - 1", "B -> A : int max", "A -> B : int max + 1"]
    out.append("autonumber stop")
    out.append("A -> B : not numbered")
    out.append("autonumber resume 1000000000000")
    out += ["A -> B : resumed, step one trillion", "B -> A : next"]
    out.append("||100000||")
    out.append("A -> B : after a space of 100000")
    out.append("@enduml")
    return "\n".join(out) + "\n"


def seq_huge_space():
    """A space of 4,294,967,295 pixels (u32 max) between two messages, and
    rows after it: their positions are past f32's exact integers."""
    out = ["@startuml", "title A space of u32::MAX", "participant A", "participant B", "participant C"]
    out.append("A -> B : before")
    out.append("||4294967295||")
    for i in range(12):
        out.append(f"{'ABC'[i % 3]} -> {'ABC'[(i + 1) % 3]} : after {i}")
    out.append("note over A, C : a note after the space")
    out.append("@enduml")
    return "\n".join(out) + "\n"


def seq_arrows():
    """Every message arrow of PlantUML, one per line (QA-28: some of them
    make the file open as a class diagram)."""
    out = ["@startuml", "participant A", "participant B"]
    for a in ["->", "-->", "->>", "-\\", "-\\\\", "-/", "-//", "//--", "\\\\-",
              "->x", "x->", "<->", "-[#red]>", "->o", "o->", "o->o", "<->o"]:
        out.append(f"A {a} B : {a}")
    out.append("@enduml")
    return "\n".join(out) + "\n"


def main():
    os.makedirs(OUT, exist_ok=True)
    write("class-many.umpl", class_many())
    # Small enough to lay out in a test, with crossings (QA-33).
    write("class-crossings.umpl", class_many(40, 80, seed=7))
    write("class-huge.umpl", class_huge())
    write("class-long-text.umpl", class_long_text())
    write("class-far-coords.umpl", class_far_coords())
    write("seq-many.puml", seq_many())
    write("seq-long-text.puml", seq_long_text())
    write("seq-big-numbers.puml", seq_big_numbers())
    write("seq-huge-space.puml", seq_huge_space())
    write("seq-arrows.puml", seq_arrows())


if __name__ == "__main__":
    main()
