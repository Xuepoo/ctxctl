package com.example.app;

import java.util.List;
import static java.lang.Math.PI;
import com.example.util.Helper;

/** A 2D point. */
public class Point {
    private double x;
    private double y;

    /** Distance from the origin. */
    public double norm() {
        return Math.sqrt(x * x + y * y);
    }
}

/** A repository interface. */
public interface Repo {
    List<String> all();
}

public class App {
    public static void main(String[] args) {}
}
