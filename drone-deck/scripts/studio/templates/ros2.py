"""Read-only PX4 ROS 2 status monitor; use matching generated px4_msgs."""
import rclpy
from rclpy.node import Node
from rclpy.qos import QoSProfile,ReliabilityPolicy,DurabilityPolicy,HistoryPolicy
from px4_msgs.msg import VehicleStatus
class Monitor(Node):
    def __init__(self):
        super().__init__('drone_deck_status')
        qos=QoSProfile(reliability=ReliabilityPolicy.BEST_EFFORT,durability=DurabilityPolicy.TRANSIENT_LOCAL,history=HistoryPolicy.KEEP_LAST,depth=1)
        version=getattr(VehicleStatus,'MESSAGE_VERSION',0)
        topic='/fmu/out/vehicle_status'+('_v'+str(version) if version else '')
        self.subscription=self.create_subscription(VehicleStatus,topic,self.status,qos)
        self.get_logger().info('Read-only subscriber: '+topic)
    def status(self,message):self.get_logger().info('arming_state='+str(message.arming_state)+' nav_state='+str(message.nav_state))
def main():
    rclpy.init();node=Monitor()
    try:rclpy.spin(node)
    finally:node.destroy_node();rclpy.shutdown()
if __name__=='__main__':main()
