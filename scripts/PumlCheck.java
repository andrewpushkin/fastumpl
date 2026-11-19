// Asks PlantUML to read diagram files, without drawing them, and reports
// what it made of each diagram. Run by scripts/puml-check.sh inside the dev
// image:
//
//   java -cp /opt/plantuml/plantuml.jar scripts/PumlCheck.java LIST OUT
//
// LIST holds one file path per line. OUT gets one line per diagram:
// path, diagram number (from 1), kind (PlantUML's class name for the
// diagram), status (ok or error), error line (in the diagram, from 1),
// and PlantUML's description of the diagram or its error message.
//
//   java -cp /opt/plantuml/plantuml.jar scripts/PumlCheck.java --dump FILE...
//
// prints what PlantUML made of the first diagram of each file: every
// group (package) and leaf (class, note…) with its qualified name, shown
// text and parent, and every link with its ends. To check how PlantUML
// reads a construct.

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;

import net.sourceforge.plantuml.BlockUml;
import net.sourceforge.plantuml.ErrorUml;
import net.sourceforge.plantuml.SourceFileReader;
import net.sourceforge.plantuml.core.Diagram;
import net.sourceforge.plantuml.error.PSystemError;

public class PumlCheck {
    static String clean(String s) {
        return s == null ? "" : s.replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
    }

    static List<String> check(String path) {
        List<String> out = new ArrayList<>();
        try {
            SourceFileReader reader = new SourceFileReader(false, new File(path));
            int n = 0;
            for (BlockUml block : reader.getBlocks()) {
                n++;
                Diagram d = block.getDiagram();
                String kind = d.getClass().getSimpleName();
                if (d instanceof PSystemError e) {
                    ErrorUml first = e.getFirstError();
                    int line = first == null || first.getLineLocation() == null
                            ? 0 : first.getLineLocation().getPosition() + 1;
                    String message = first == null ? "" : first.getError();
                    out.add(String.join("\t", path, "" + n, kind, "error", "" + line, clean(message)));
                } else {
                    String description = d.getDescription() == null ? "" : d.getDescription().getDescription();
                    out.add(String.join("\t", path, "" + n, kind, "ok", "0", clean(description)));
                }
            }
        } catch (Throwable t) {
            out.add(String.join("\t", path, "0", "-", "crash", "0", clean(t.toString())));
        }
        return out;
    }

    static String shown(net.sourceforge.plantuml.klimt.creole.Display d) {
        if (d == null) {
            return "";
        }
        List<String> parts = new ArrayList<>();
        for (CharSequence c : d.asList()) {
            parts.add(c.toString());
        }
        return clean(String.join("\\n", parts));
    }

    static String name(net.sourceforge.plantuml.abel.Entity e) {
        return e == null ? "-" : e.getQuark().getQualifiedName();
    }

    static void dump(String path) throws Exception {
        SourceFileReader reader = new SourceFileReader(false, new File(path));
        Diagram d = reader.getBlocks().get(0).getDiagram();
        System.out.println("== " + path + ": " + d.getClass().getSimpleName());
        if (!(d instanceof net.atmp.CucaDiagram c)) {
            return;
        }
        for (var g : c.groups()) {
            System.out.println("group\t" + name(g) + "\t" + shown(g.getDisplay()) + "\tin " + name(g.getParentContainer()));
        }
        for (var l : c.leafs()) {
            System.out.println("leaf\t" + name(l) + "\t" + shown(l.getDisplay()) + "\tin " + name(l.getParentContainer())
                    + "\t" + l.getLeafType() + (c.isRemoved(l) ? "\tremoved" : ""));
        }
        for (var k : c.getLinks()) {
            System.out.println("link\t" + name(k.getEntity1()) + "\t" + name(k.getEntity2()) + "\t" + shown(k.getLabel())
                    + "\t" + k.getQuantifier1() + "\t" + k.getQuantifier2());
        }
    }

    public static void main(String[] args) throws Exception {
        if (args.length > 0 && args[0].equals("--dump")) {
            for (int i = 1; i < args.length; i++) {
                dump(args[i]);
            }
            return;
        }
        List<String> files = Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8);
        int threads = Math.max(1, Runtime.getRuntime().availableProcessors() / 2);
        ExecutorService pool = Executors.newFixedThreadPool(threads);
        List<Future<List<String>>> results = new ArrayList<>();
        for (String f : files) {
            if (!f.isBlank()) {
                results.add(pool.submit(() -> check(f)));
            }
        }
        try (PrintWriter w = new PrintWriter(args[1], StandardCharsets.UTF_8)) {
            int done = 0;
            for (Future<List<String>> r : results) {
                for (String line : r.get()) {
                    w.println(line);
                }
                if (++done % 2000 == 0) {
                    System.err.println("  " + done + " / " + results.size());
                }
            }
        }
        pool.shutdown();
        pool.awaitTermination(1, TimeUnit.MINUTES);
        System.exit(0);
    }
}
