using System;
using System.Collections.Generic;
using static System.Math;

namespace Demo.App {
    /// A 2D point.
    public class Point {
        private double x;
        private double y;

        /// Distance from the origin.
        public double Norm() {
            return Sqrt(x * x + y * y);
        }
    }

    public interface IRepo {
        List<string> All();
    }

    public record Pair(int A, int B);

    public enum Status { On, Off }

    public struct Vector2 {
        public double X;
    }
}
